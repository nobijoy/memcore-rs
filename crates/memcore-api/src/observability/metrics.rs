use std::sync::atomic::{AtomicU64, Ordering};

/// In-process counters exposed at `GET /metrics` (per API process only).
#[derive(Debug, Default)]
pub struct Metrics {
    http_requests_total: AtomicU64,
    api_errors_total: AtomicU64,
    request_duration_ms_sum: AtomicU64,
    request_duration_ms_count: AtomicU64,
    memory_add_requests_total: AtomicU64,
    memory_search_requests_total: AtomicU64,
    context_requests_total: AtomicU64,
}

impl Metrics {
    pub fn record_request(&self, path: &str, status: u16, latency_ms: u64) {
        self.http_requests_total.fetch_add(1, Ordering::Relaxed);
        self.request_duration_ms_sum
            .fetch_add(latency_ms, Ordering::Relaxed);
        self.request_duration_ms_count
            .fetch_add(1, Ordering::Relaxed);

        if status >= 400 {
            self.api_errors_total.fetch_add(1, Ordering::Relaxed);
        }

        match path {
            "/api/v1/memories" => {
                self.memory_add_requests_total.fetch_add(1, Ordering::Relaxed);
            }
            "/api/v1/memories/search" => {
                self.memory_search_requests_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            "/api/v1/context" => {
                self.context_requests_total.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    pub fn render_prometheus(&self) -> String {
        format!(
            concat!(
                "# HELP memcore_http_requests_total Total HTTP requests handled by this process.\n",
                "# TYPE memcore_http_requests_total counter\n",
                "memcore_http_requests_total {}\n",
                "# HELP memcore_api_errors_total Total API error responses (status >= 400).\n",
                "# TYPE memcore_api_errors_total counter\n",
                "memcore_api_errors_total {}\n",
                "# HELP memcore_request_duration_ms_sum Sum of request durations in milliseconds.\n",
                "# TYPE memcore_request_duration_ms_sum counter\n",
                "memcore_request_duration_ms_sum {}\n",
                "# HELP memcore_request_duration_ms_count Count of requests included in duration sum.\n",
                "# TYPE memcore_request_duration_ms_count counter\n",
                "memcore_request_duration_ms_count {}\n",
                "# HELP memcore_memory_add_requests_total Total POST /api/v1/memories requests.\n",
                "# TYPE memcore_memory_add_requests_total counter\n",
                "memcore_memory_add_requests_total {}\n",
                "# HELP memcore_memory_search_requests_total Total POST /api/v1/memories/search requests.\n",
                "# TYPE memcore_memory_search_requests_total counter\n",
                "memcore_memory_search_requests_total {}\n",
                "# HELP memcore_context_requests_total Total POST /api/v1/context requests.\n",
                "# TYPE memcore_context_requests_total counter\n",
                "memcore_context_requests_total {}\n",
            ),
            self.http_requests_total.load(Ordering::Relaxed),
            self.api_errors_total.load(Ordering::Relaxed),
            self.request_duration_ms_sum.load(Ordering::Relaxed),
            self.request_duration_ms_count.load(Ordering::Relaxed),
            self.memory_add_requests_total.load(Ordering::Relaxed),
            self.memory_search_requests_total.load(Ordering::Relaxed),
            self.context_requests_total.load(Ordering::Relaxed),
        )
    }
}
