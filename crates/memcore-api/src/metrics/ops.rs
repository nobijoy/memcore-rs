//! Application-level operation counters (safe labels only).

use metrics::counter;

pub fn record_memory_create(status: &str) {
    counter!("memcore_memory_create_total", "status" => status.to_string()).increment(1);
}

pub fn record_memory_delete(status: &str) {
    counter!("memcore_memory_delete_total", "status" => status.to_string()).increment(1);
}

pub fn record_memory_forget_user(status: &str) {
    counter!("memcore_memory_forget_user_total", "status" => status.to_string()).increment(1);
}

pub fn record_memory_search(status: &str) {
    counter!("memcore_memory_search_total", "status" => status.to_string()).increment(1);
}

pub fn record_context_request(status: &str) {
    counter!("memcore_context_requests_total", "status" => status.to_string()).increment(1);
}

pub fn record_import_request(status: &str) {
    counter!("memcore_import_requests_total", "status" => status.to_string()).increment(1);
}

pub fn record_export_request(status: &str) {
    counter!("memcore_export_requests_total", "status" => status.to_string()).increment(1);
}

pub fn record_quota_rejection(quota_type: &str) {
    counter!(
        "memcore_quota_rejections_total",
        "quota_type" => quota_type.to_string()
    )
    .increment(1);
}

pub fn record_provider_request(provider: &str, model: &str, operation: &str, status: &str) {
    counter!(
        "memcore_provider_requests_total",
        "provider" => provider.to_string(),
        "model" => model.to_string(),
        "operation" => operation.to_string(),
        "status" => status.to_string(),
    )
    .increment(1);
}

pub fn record_provider_failure(provider: &str, error_class: &str) {
    counter!(
        "memcore_provider_failures_total",
        "provider" => provider.to_string(),
        "error_class" => error_class.to_string(),
    )
    .increment(1);
}

pub fn record_provider_retries(provider: &str, count: u64) {
    if count == 0 {
        return;
    }
    counter!(
        "memcore_provider_retries_total",
        "provider" => provider.to_string(),
    )
    .increment(count);
}

pub fn record_provider_circuit_open(provider: &str) {
    counter!(
        "memcore_provider_circuit_breaker_open_total",
        "provider" => provider.to_string(),
    )
    .increment(1);
}

pub fn record_context_cache_hit(backend: &str) {
    counter!(
        "memcore_context_cache_hits_total",
        "backend" => backend.to_string()
    )
    .increment(1);
}

pub fn record_context_cache_miss(backend: &str) {
    counter!(
        "memcore_context_cache_misses_total",
        "backend" => backend.to_string()
    )
    .increment(1);
}

pub fn record_context_cache_stale_hit(backend: &str) {
    counter!(
        "memcore_context_cache_stale_hits_total",
        "backend" => backend.to_string()
    )
    .increment(1);
}

pub fn record_context_cache_refresh(backend: &str, status: &str) {
    if status == "failed" {
        counter!(
            "memcore_context_cache_refresh_failures_total",
            "backend" => backend.to_string()
        )
        .increment(1);
    }
    counter!(
        "memcore_context_cache_refresh_total",
        "backend" => backend.to_string(),
        "status" => status.to_string(),
    )
    .increment(1);
}

pub fn record_background_job_run(job_kind: &str, status: &str, duration_secs: f64) {
    counter!(
        "memcore_background_job_runs_total",
        "job_kind" => job_kind.to_string(),
        "status" => status.to_string(),
    )
    .increment(1);
    metrics::histogram!(
        "memcore_background_job_duration_seconds",
        "job_kind" => job_kind.to_string(),
    )
    .record(duration_secs);

    if status.eq_ignore_ascii_case("Failed") || status.eq_ignore_ascii_case("failed") {
        counter!(
            "memcore_background_job_failures_total",
            "job_kind" => job_kind.to_string(),
        )
        .increment(1);
    }
    if status.eq_ignore_ascii_case("Skipped") || status.eq_ignore_ascii_case("skipped") {
        counter!(
            "memcore_background_job_lock_skips_total",
            "job_kind" => job_kind.to_string(),
        )
        .increment(1);
    }
    if status.eq_ignore_ascii_case("Cancelled") || status.eq_ignore_ascii_case("cancelled") {
        counter!(
            "memcore_background_job_cancelled_total",
            "job_kind" => job_kind.to_string(),
        )
        .increment(1);
    }
}

pub fn record_background_job_retries(job_kind: &str, retries: u64) {
    if retries == 0 {
        return;
    }
    counter!(
        "memcore_background_job_retries_total",
        "job_kind" => job_kind.to_string(),
    )
    .increment(retries);
}
