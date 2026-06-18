use async_trait::async_trait;
use chrono::Utc;
use memcore_common::{MemcoreError, MemcoreResult};
use sqlx::Row;
use sqlx::postgres::{PgPool, PgRow};

use super::types::{
    AppliedMigration, Migration, MigrationRunner, MigrationValidationReport, sorted_migrations,
    split_sql_statements, validate_applied_migrations,
};

fn migration_error(context: impl Into<String>, error: impl std::fmt::Display) -> MemcoreError {
    MemcoreError::MigrationError(format!("{}: {error}", context.into()))
}

fn parse_applied_row(row: &PgRow) -> MemcoreResult<AppliedMigration> {
    Ok(AppliedMigration {
        version: row
            .try_get("version")
            .map_err(|error| migration_error("read postgres migration version", error))?,
        name: row
            .try_get("name")
            .map_err(|error| migration_error("read postgres migration name", error))?,
        checksum: row
            .try_get("checksum")
            .map_err(|error| migration_error("read postgres migration checksum", error))?,
        applied_at: row
            .try_get("applied_at")
            .map_err(|error| migration_error("read postgres migration applied_at", error))?,
    })
}

#[derive(Debug, Clone)]
pub struct PostgresMigrationRunner {
    pool: PgPool,
}

impl PostgresMigrationRunner {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> PgPool {
        self.pool.clone()
    }

    async fn table_exists(&self, table_name: &str) -> MemcoreResult<bool> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(1) FROM information_schema.tables WHERE table_schema = 'public' AND table_name = $1",
        )
        .bind(table_name)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| migration_error("check postgres table existence", error))?;
        Ok(exists > 0)
    }

    async fn schema_migration_count(&self) -> MemcoreResult<i64> {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(1) FROM schema_migrations")
            .fetch_one(&self.pool)
            .await
            .map_err(|error| migration_error("count postgres schema migrations", error))
    }

    async fn seed_from_sqlx_migrations(&self, migrations: &[Migration]) -> MemcoreResult<()> {
        if self.schema_migration_count().await? > 0
            || !self.table_exists("_sqlx_migrations").await?
        {
            return Ok(());
        }

        let legacy_versions =
            sqlx::query("SELECT version FROM _sqlx_migrations WHERE success = true")
                .fetch_all(&self.pool)
                .await
                .map_err(|error| migration_error("read postgres legacy sqlx migrations", error))?
                .into_iter()
                .map(|row| row.try_get::<i64, _>("version"))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| {
                    migration_error("read postgres legacy migration version", error)
                })?;

        if legacy_versions.is_empty() {
            return Ok(());
        }

        let legacy_versions = legacy_versions
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        for migration in sorted_migrations(migrations)?
            .into_iter()
            .filter(|migration| legacy_versions.contains(&migration.version))
        {
            sqlx::query(
                "INSERT INTO schema_migrations (version, name, checksum, applied_at) VALUES ($1, $2, $3, $4) ON CONFLICT (version) DO NOTHING",
            )
            .bind(migration.version)
            .bind(migration.name)
            .bind(&migration.checksum)
            .bind(Utc::now())
            .execute(&self.pool)
            .await
            .map_err(|error| migration_error("seed postgres schema migrations", error))?;
        }

        Ok(())
    }
}

#[async_trait]
impl MigrationRunner for PostgresMigrationRunner {
    async fn ensure_migration_table(&self) -> MemcoreResult<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version BIGINT PRIMARY KEY,
                name TEXT NOT NULL,
                checksum TEXT NOT NULL,
                applied_at TIMESTAMPTZ NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|error| migration_error("create postgres schema_migrations table", error))?;
        Ok(())
    }

    async fn applied_migrations(&self) -> MemcoreResult<Vec<AppliedMigration>> {
        self.ensure_migration_table().await?;
        let rows = sqlx::query(
            "SELECT version, name, checksum, applied_at FROM schema_migrations ORDER BY version ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|error| migration_error("read postgres schema migrations", error))?;

        rows.iter().map(parse_applied_row).collect()
    }

    async fn validate_migrations(
        &self,
        migrations: &[Migration],
    ) -> MemcoreResult<MigrationValidationReport> {
        self.ensure_migration_table().await?;
        self.seed_from_sqlx_migrations(migrations).await?;
        validate_applied_migrations(migrations, &self.applied_migrations().await?)
    }

    async fn apply_pending_migrations(
        &self,
        migrations: &[Migration],
    ) -> MemcoreResult<MigrationValidationReport> {
        self.ensure_migration_table().await?;
        self.seed_from_sqlx_migrations(migrations).await?;

        let report = self.validate_migrations(migrations).await?;
        if report.has_blocking_issues() {
            return Err(MemcoreError::MigrationError(
                "migration validation failed before applying pending migrations".to_string(),
            ));
        }

        let applied = self.applied_migrations().await?;
        let applied_versions = applied
            .iter()
            .map(|migration| migration.version)
            .collect::<std::collections::HashSet<_>>();

        for migration in sorted_migrations(migrations)?
            .into_iter()
            .filter(|migration| !applied_versions.contains(&migration.version))
        {
            let mut tx =
                self.pool.begin().await.map_err(|error| {
                    migration_error("begin postgres migration transaction", error)
                })?;

            for statement in split_sql_statements(migration.sql) {
                sqlx::query(&statement)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| {
                        migration_error("apply postgres migration statement", error)
                    })?;
            }

            sqlx::query(
                "INSERT INTO schema_migrations (version, name, checksum, applied_at) VALUES ($1, $2, $3, $4)",
            )
            .bind(migration.version)
            .bind(migration.name)
            .bind(&migration.checksum)
            .bind(Utc::now())
            .execute(&mut *tx)
            .await
            .map_err(|error| migration_error("record postgres migration", error))?;

            tx.commit()
                .await
                .map_err(|error| migration_error("commit postgres migration transaction", error))?;
        }

        self.validate_migrations(migrations).await
    }
}

pub async fn required_postgres_tables_exist(pool: &PgPool) -> MemcoreResult<Vec<String>> {
    let required = [
        "facts",
        "memory_events",
        "api_keys",
        "provider_usage_events",
        "org_plan_configs",
        "memory_usage_snapshots",
        "background_job_runs",
        "background_job_locks",
        "schema_migrations",
    ];

    let runner = PostgresMigrationRunner::new(pool.clone());
    let mut missing = Vec::new();
    for table in required {
        if !runner.table_exists(table).await? {
            missing.push(table.to_string());
        }
    }
    Ok(missing)
}

pub async fn run_postgres_migrations(pool: &PgPool) -> MemcoreResult<MigrationValidationReport> {
    let runner = PostgresMigrationRunner::new(pool.clone());
    runner
        .apply_pending_migrations(&crate::migrations::postgres_migrations())
        .await
}
