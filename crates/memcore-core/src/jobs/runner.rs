use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use memcore_common::{MemcoreError, MemcoreResult};
use tokio::time::{MissedTickBehavior, interval, timeout};

use super::registry::BackgroundJob;
use super::types::{
    BackgroundJobDefinition, BackgroundJobKind, BackgroundJobRun, BackgroundJobSnapshot,
    BackgroundJobStatus,
};
use crate::ports::{BackgroundJobRunStore, StoredBackgroundJobRun};

const DEFAULT_RECENT_RUN_LIMIT: usize = 100;

#[derive(Debug)]
struct BackgroundJobStateInner {
    last_runs: HashMap<BackgroundJobKind, DateTime<Utc>>,
    running: HashSet<BackgroundJobKind>,
    recent_runs: VecDeque<BackgroundJobRun>,
}

#[derive(Debug)]
pub struct InMemoryBackgroundJobState {
    inner: Mutex<BackgroundJobStateInner>,
    recent_run_limit: usize,
}

impl Default for InMemoryBackgroundJobState {
    fn default() -> Self {
        Self::new(DEFAULT_RECENT_RUN_LIMIT)
    }
}

impl InMemoryBackgroundJobState {
    pub fn new(recent_run_limit: usize) -> Self {
        Self {
            inner: Mutex::new(BackgroundJobStateInner {
                last_runs: HashMap::new(),
                running: HashSet::new(),
                recent_runs: VecDeque::new(),
            }),
            recent_run_limit: recent_run_limit.max(1),
        }
    }

    pub fn recent_runs(&self) -> Vec<BackgroundJobRun> {
        self.inner
            .lock()
            .expect("background job state mutex should not be poisoned")
            .recent_runs
            .iter()
            .cloned()
            .collect()
    }

    pub fn is_running(&self, kind: BackgroundJobKind) -> bool {
        self.inner
            .lock()
            .expect("background job state mutex should not be poisoned")
            .running
            .contains(&kind)
    }

    pub fn try_begin(&self, kind: BackgroundJobKind) -> bool {
        let mut inner = self
            .inner
            .lock()
            .expect("background job state mutex should not be poisoned");
        inner.running.insert(kind)
    }

    pub fn is_due(&self, definition: &BackgroundJobDefinition, now: DateTime<Utc>) -> bool {
        let inner = self
            .inner
            .lock()
            .expect("background job state mutex should not be poisoned");

        if inner.running.contains(&definition.kind) {
            return false;
        }

        let Some(last_run) = inner.last_runs.get(&definition.kind) else {
            return true;
        };

        let Ok(interval) = chrono::Duration::from_std(definition.interval) else {
            return false;
        };

        *last_run + interval <= now
    }

    pub fn record_run(&self, run: BackgroundJobRun) {
        let mut inner = self
            .inner
            .lock()
            .expect("background job state mutex should not be poisoned");
        inner.running.remove(&run.kind);
        inner.last_runs.insert(run.kind, run.started_at);
        inner.recent_runs.push_front(run);
        while inner.recent_runs.len() > self.recent_run_limit {
            inner.recent_runs.pop_back();
        }
    }
}

#[derive(Clone)]
pub struct BackgroundJobRunner {
    jobs_enabled: bool,
    runner_interval: Duration,
    jobs: HashMap<BackgroundJobKind, Arc<dyn BackgroundJob>>,
    state: Arc<InMemoryBackgroundJobState>,
    history_enabled: bool,
    run_store: Option<Arc<dyn BackgroundJobRunStore>>,
}

impl BackgroundJobRunner {
    pub fn new(
        jobs_enabled: bool,
        runner_interval: Duration,
        jobs: Vec<Arc<dyn BackgroundJob>>,
    ) -> Self {
        Self::with_state(
            jobs_enabled,
            runner_interval,
            jobs,
            Arc::new(InMemoryBackgroundJobState::default()),
        )
    }

    pub fn with_state(
        jobs_enabled: bool,
        runner_interval: Duration,
        jobs: Vec<Arc<dyn BackgroundJob>>,
        state: Arc<InMemoryBackgroundJobState>,
    ) -> Self {
        Self {
            jobs_enabled,
            runner_interval,
            jobs: jobs.into_iter().map(|job| (job.kind(), job)).collect(),
            state,
            history_enabled: false,
            run_store: None,
        }
    }

    pub fn with_history_store(
        mut self,
        history_enabled: bool,
        run_store: Option<Arc<dyn BackgroundJobRunStore>>,
    ) -> Self {
        self.history_enabled = history_enabled;
        self.run_store = run_store;
        self
    }

    pub fn snapshot(&self) -> BackgroundJobSnapshot {
        let mut jobs = self
            .jobs
            .values()
            .map(|job| job.definition())
            .collect::<Vec<_>>();
        jobs.sort_by_key(|definition| definition.kind.as_str());

        BackgroundJobSnapshot {
            jobs_enabled: self.jobs_enabled,
            jobs,
            recent_runs: self.state.recent_runs(),
        }
    }

    pub fn state(&self) -> Arc<InMemoryBackgroundJobState> {
        self.state.clone()
    }

    pub async fn run_due_once(&self) -> Vec<BackgroundJobRun> {
        if !self.jobs_enabled {
            return Vec::new();
        }

        let now = Utc::now();
        let mut runs = Vec::new();
        for job in self.jobs.values() {
            let definition = job.definition();
            if !definition.enabled || !self.state.is_due(&definition, now) {
                continue;
            }
            runs.push(self.run_job(job.clone()).await);
        }
        runs
    }

    pub async fn run_manual(&self, kind: BackgroundJobKind) -> MemcoreResult<BackgroundJobRun> {
        let job = self.jobs.get(&kind).cloned().ok_or_else(|| {
            MemcoreError::ValidationError(format!("job is not registered: {}", kind.as_str()))
        })?;
        Ok(self.run_job(job).await)
    }

    async fn run_job(&self, job: Arc<dyn BackgroundJob>) -> BackgroundJobRun {
        let kind = job.kind();
        if !self.state.try_begin(kind) {
            let run = BackgroundJobRun::skipped(kind, "job is already running");
            tracing::warn!(
                job_kind = %kind,
                status = BackgroundJobStatus::Skipped.as_str(),
                "background job overlapping run skipped"
            );
            self.record_run(run.clone()).await;
            return run;
        }

        tracing::info!(job_kind = %kind, "background job started");
        let run = match timeout(job.interval(), job.run_once()).await {
            Ok(Ok(run)) => run,
            Ok(Err(error)) => {
                let mut run = BackgroundJobRun::failed(kind, error.code(), "background job failed");
                run.error_message = Some(error.message());
                run
            }
            Err(_) => BackgroundJobRun::failed(kind, "TIMEOUT", "background job timed out"),
        };

        let status = run.status;
        tracing::info!(
            job_kind = %kind,
            status = status.as_str(),
            duration_ms = run.duration_ms,
            org_count = run.org_count,
            affected_count = run.affected_count,
            error_code = run.error_code.as_deref(),
            "background job finished"
        );
        self.record_run(run.clone()).await;
        run
    }

    async fn record_run(&self, run: BackgroundJobRun) {
        self.state.record_run(run.clone());
        if !self.history_enabled {
            return;
        }

        let Some(store) = &self.run_store else {
            return;
        };

        let stored = StoredBackgroundJobRun::from(run.clone());
        match store.insert_run(stored).await {
            Ok(_) => {
                tracing::info!(
                    job_kind = %run.kind,
                    status = run.status.as_str(),
                    duration_ms = run.duration_ms,
                    run_id = %run.id,
                    "background job run persisted"
                );
            }
            Err(error) => {
                tracing::warn!(
                    job_kind = %run.kind,
                    status = run.status.as_str(),
                    duration_ms = run.duration_ms,
                    run_id = %run.id,
                    error_code = error.code(),
                    "background job history persistence failed"
                );
            }
        }
    }

    pub async fn run_forever(&self) {
        if !self.jobs_enabled {
            tracing::info!("background job runner disabled");
            return;
        }

        tracing::info!(
            runner_interval_seconds = self.runner_interval.as_secs(),
            "background job runner started"
        );

        let mut ticker = interval(self.runner_interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            ticker.tick().await;
            let _ = self.run_due_once().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::ports::{
        BackgroundJobRunQuery, BackgroundJobRunQueryResult, BackgroundJobRunStore,
        StoredBackgroundJobRun,
    };

    struct TestJob {
        kind: BackgroundJobKind,
        enabled: bool,
        interval: Duration,
        runs: Arc<AtomicUsize>,
        fail: bool,
    }

    #[derive(Debug, Default)]
    struct CapturingRunStore {
        runs: Mutex<Vec<StoredBackgroundJobRun>>,
        fail_insert: bool,
    }

    #[async_trait]
    impl BackgroundJobRunStore for CapturingRunStore {
        async fn insert_run(
            &self,
            run: StoredBackgroundJobRun,
        ) -> MemcoreResult<StoredBackgroundJobRun> {
            if self.fail_insert {
                return Err(MemcoreError::StorageError(
                    "test persistence failure".to_string(),
                ));
            }
            self.runs
                .lock()
                .expect("capturing store mutex should not be poisoned")
                .push(run.clone());
            Ok(run)
        }

        async fn query_runs(
            &self,
            _query: BackgroundJobRunQuery,
        ) -> MemcoreResult<BackgroundJobRunQueryResult> {
            Ok(BackgroundJobRunQueryResult {
                runs: self
                    .runs
                    .lock()
                    .expect("capturing store mutex should not be poisoned")
                    .clone(),
                next_cursor: None,
            })
        }

        async fn delete_runs_older_than(
            &self,
            _cutoff: DateTime<Utc>,
            _dry_run: bool,
        ) -> MemcoreResult<usize> {
            Ok(0)
        }
    }

    #[async_trait]
    impl BackgroundJob for TestJob {
        fn kind(&self) -> BackgroundJobKind {
            self.kind
        }

        fn interval(&self) -> Duration {
            self.interval
        }

        fn enabled(&self) -> bool {
            self.enabled
        }

        async fn run_once(&self) -> MemcoreResult<BackgroundJobRun> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(MemcoreError::Internal("test failure".to_string()));
            }
            Ok(BackgroundJobRun::running(self.kind).finish(BackgroundJobStatus::Succeeded))
        }
    }

    #[test]
    fn state_starts_empty_and_tracks_running() {
        let state = InMemoryBackgroundJobState::default();
        assert!(state.recent_runs().is_empty());
        assert!(!state.is_running(BackgroundJobKind::MemoryUsageSnapshot));
        assert!(state.try_begin(BackgroundJobKind::MemoryUsageSnapshot));
        assert!(state.is_running(BackgroundJobKind::MemoryUsageSnapshot));
        assert!(!state.try_begin(BackgroundJobKind::MemoryUsageSnapshot));
        assert!(state.try_begin(BackgroundJobKind::ProviderUsageRetention));
    }

    #[test]
    fn state_records_runs_and_respects_recent_limit() {
        let state = InMemoryBackgroundJobState::new(2);
        for _ in 0..3 {
            assert!(state.try_begin(BackgroundJobKind::MemoryUsageSnapshot));
            state.record_run(
                BackgroundJobRun::running(BackgroundJobKind::MemoryUsageSnapshot)
                    .finish(BackgroundJobStatus::Succeeded),
            );
        }

        assert_eq!(state.recent_runs().len(), 2);
        assert!(!state.is_running(BackgroundJobKind::MemoryUsageSnapshot));
    }

    #[tokio::test]
    async fn disabled_runner_does_not_run_jobs() {
        let runs = Arc::new(AtomicUsize::new(0));
        let runner = BackgroundJobRunner::new(
            false,
            Duration::from_millis(10),
            vec![Arc::new(TestJob {
                kind: BackgroundJobKind::MemoryUsageSnapshot,
                enabled: true,
                interval: Duration::from_secs(1),
                runs: runs.clone(),
                fail: false,
            })],
        );

        assert!(runner.run_due_once().await.is_empty());
        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn enabled_runner_runs_due_jobs_and_skips_disabled_jobs() {
        let enabled_runs = Arc::new(AtomicUsize::new(0));
        let disabled_runs = Arc::new(AtomicUsize::new(0));
        let runner = BackgroundJobRunner::new(
            true,
            Duration::from_millis(10),
            vec![
                Arc::new(TestJob {
                    kind: BackgroundJobKind::MemoryUsageSnapshot,
                    enabled: true,
                    interval: Duration::from_secs(1),
                    runs: enabled_runs.clone(),
                    fail: false,
                }),
                Arc::new(TestJob {
                    kind: BackgroundJobKind::ProviderUsageRetention,
                    enabled: false,
                    interval: Duration::from_secs(1),
                    runs: disabled_runs.clone(),
                    fail: false,
                }),
            ],
        );

        let runs = runner.run_due_once().await;
        assert_eq!(runs.len(), 1);
        assert_eq!(enabled_runs.load(Ordering::SeqCst), 1);
        assert_eq!(disabled_runs.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_job_does_not_stop_runner() {
        let failed_runs = Arc::new(AtomicUsize::new(0));
        let successful_runs = Arc::new(AtomicUsize::new(0));
        let runner = BackgroundJobRunner::new(
            true,
            Duration::from_millis(10),
            vec![
                Arc::new(TestJob {
                    kind: BackgroundJobKind::MemoryUsageSnapshot,
                    enabled: true,
                    interval: Duration::from_secs(1),
                    runs: failed_runs.clone(),
                    fail: true,
                }),
                Arc::new(TestJob {
                    kind: BackgroundJobKind::ProviderUsageRetention,
                    enabled: true,
                    interval: Duration::from_secs(1),
                    runs: successful_runs.clone(),
                    fail: false,
                }),
            ],
        );

        let runs = runner.run_due_once().await;
        assert_eq!(runs.len(), 2);
        assert_eq!(failed_runs.load(Ordering::SeqCst), 1);
        assert_eq!(successful_runs.load(Ordering::SeqCst), 1);
        assert!(
            runs.iter()
                .any(|run| run.status == BackgroundJobStatus::Failed)
        );
        assert!(
            runs.iter()
                .any(|run| run.status == BackgroundJobStatus::Succeeded)
        );
    }

    #[tokio::test]
    async fn overlapping_same_job_is_skipped() {
        let runner = BackgroundJobRunner::new(true, Duration::from_millis(10), Vec::new());
        assert!(
            runner
                .state
                .try_begin(BackgroundJobKind::MemoryUsageSnapshot)
        );

        let run = runner
            .run_job(Arc::new(TestJob {
                kind: BackgroundJobKind::MemoryUsageSnapshot,
                enabled: true,
                interval: Duration::from_secs(1),
                runs: Arc::new(AtomicUsize::new(0)),
                fail: false,
            }))
            .await;

        assert_eq!(run.status, BackgroundJobStatus::Skipped);
    }

    #[tokio::test]
    async fn manual_run_works_when_global_runner_is_disabled() {
        let runs = Arc::new(AtomicUsize::new(0));
        let runner = BackgroundJobRunner::new(
            false,
            Duration::from_millis(10),
            vec![Arc::new(TestJob {
                kind: BackgroundJobKind::MemoryUsageSnapshot,
                enabled: false,
                interval: Duration::from_secs(1),
                runs: runs.clone(),
                fail: false,
            })],
        );

        let run = runner
            .run_manual(BackgroundJobKind::MemoryUsageSnapshot)
            .await
            .expect("manual run should work");
        assert_eq!(run.status, BackgroundJobStatus::Succeeded);
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn successful_manual_run_is_persisted_when_history_store_is_configured() {
        let runs = Arc::new(AtomicUsize::new(0));
        let store = Arc::new(CapturingRunStore::default());
        let runner = BackgroundJobRunner::new(
            false,
            Duration::from_millis(10),
            vec![Arc::new(TestJob {
                kind: BackgroundJobKind::MemoryUsageSnapshot,
                enabled: false,
                interval: Duration::from_secs(1),
                runs: runs.clone(),
                fail: false,
            })],
        )
        .with_history_store(true, Some(store.clone()));

        let run = runner
            .run_manual(BackgroundJobKind::MemoryUsageSnapshot)
            .await
            .expect("manual run should work");
        assert_eq!(run.status, BackgroundJobStatus::Succeeded);
        assert_eq!(
            store
                .runs
                .lock()
                .expect("capturing store mutex should not be poisoned")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn skipped_overlap_run_is_persisted() {
        let store = Arc::new(CapturingRunStore::default());
        let runner = BackgroundJobRunner::new(true, Duration::from_millis(10), Vec::new())
            .with_history_store(true, Some(store.clone()));
        assert!(
            runner
                .state
                .try_begin(BackgroundJobKind::MemoryUsageSnapshot)
        );

        let run = runner
            .run_job(Arc::new(TestJob {
                kind: BackgroundJobKind::MemoryUsageSnapshot,
                enabled: true,
                interval: Duration::from_secs(1),
                runs: Arc::new(AtomicUsize::new(0)),
                fail: false,
            }))
            .await;

        assert_eq!(run.status, BackgroundJobStatus::Skipped);
        assert_eq!(
            store
                .runs
                .lock()
                .expect("capturing store mutex should not be poisoned")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn persistence_failure_does_not_fail_job_execution_or_memory_state() {
        let runs = Arc::new(AtomicUsize::new(0));
        let store = Arc::new(CapturingRunStore {
            runs: Mutex::new(Vec::new()),
            fail_insert: true,
        });
        let runner = BackgroundJobRunner::new(
            false,
            Duration::from_millis(10),
            vec![Arc::new(TestJob {
                kind: BackgroundJobKind::MemoryUsageSnapshot,
                enabled: false,
                interval: Duration::from_secs(1),
                runs,
                fail: false,
            })],
        )
        .with_history_store(true, Some(store));

        let run = runner
            .run_manual(BackgroundJobKind::MemoryUsageSnapshot)
            .await
            .expect("job execution should not fail due to persistence");

        assert_eq!(run.status, BackgroundJobStatus::Succeeded);
        assert_eq!(runner.snapshot().recent_runs.len(), 1);
    }
}
