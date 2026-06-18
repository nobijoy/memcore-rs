use chrono::{DateTime, Utc};
use memcore_common::MemcoreResult;
use memcore_core::{
    BackgroundJobDefinition, BackgroundJobKind, BackgroundJobRun, BackgroundJobSnapshot,
};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackgroundJobsResponse {
    pub status: &'static str,
    pub jobs: BackgroundJobSnapshotResponse,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackgroundJobSnapshotResponse {
    pub jobs_enabled: bool,
    pub jobs: Vec<BackgroundJobDefinitionResponse>,
    pub recent_runs: Vec<BackgroundJobRunResponse>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackgroundJobDefinitionResponse {
    pub kind: String,
    pub enabled: bool,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct BackgroundJobRunResponse {
    pub id: Uuid,
    pub kind: String,
    pub status: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub org_count: u64,
    pub affected_count: u64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct RunBackgroundJobResponse {
    pub status: &'static str,
    pub run: BackgroundJobRunResponse,
}

impl From<BackgroundJobDefinition> for BackgroundJobDefinitionResponse {
    fn from(definition: BackgroundJobDefinition) -> Self {
        Self {
            kind: definition.kind.as_str().to_string(),
            enabled: definition.enabled,
            interval_seconds: definition.interval.as_secs(),
        }
    }
}

impl From<BackgroundJobRun> for BackgroundJobRunResponse {
    fn from(run: BackgroundJobRun) -> Self {
        Self {
            id: run.id,
            kind: run.kind.as_str().to_string(),
            status: run.status.as_str().to_string(),
            started_at: run.started_at,
            finished_at: run.finished_at,
            duration_ms: run.duration_ms,
            error_code: run.error_code,
            error_message: run.error_message,
            org_count: run.org_count,
            affected_count: run.affected_count,
        }
    }
}

impl From<BackgroundJobSnapshot> for BackgroundJobSnapshotResponse {
    fn from(snapshot: BackgroundJobSnapshot) -> Self {
        Self {
            jobs_enabled: snapshot.jobs_enabled,
            jobs: snapshot
                .jobs
                .into_iter()
                .map(BackgroundJobDefinitionResponse::from)
                .collect(),
            recent_runs: snapshot
                .recent_runs
                .into_iter()
                .map(BackgroundJobRunResponse::from)
                .collect(),
        }
    }
}

pub fn background_jobs_response(snapshot: BackgroundJobSnapshot) -> BackgroundJobsResponse {
    BackgroundJobsResponse {
        status: "success",
        jobs: BackgroundJobSnapshotResponse::from(snapshot),
    }
}

pub fn run_background_job_response(run: BackgroundJobRun) -> RunBackgroundJobResponse {
    RunBackgroundJobResponse {
        status: "success",
        run: BackgroundJobRunResponse::from(run),
    }
}

pub fn parse_background_job_kind(value: &str) -> MemcoreResult<BackgroundJobKind> {
    value.parse()
}
