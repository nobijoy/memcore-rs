//! Provider usage recorder that also emits Prometheus counters.

use std::sync::Arc;

use memcore_providers::{
    ProviderCallStatus, ProviderUsageEvent, ProviderUsageRecorder, ProviderUsageSnapshot,
};

use super::ops::{
    record_provider_circuit_open, record_provider_failure, record_provider_request,
    record_provider_retries,
};

pub struct PrometheusProviderUsageRecorder {
    inner: Arc<dyn ProviderUsageRecorder>,
}

impl PrometheusProviderUsageRecorder {
    pub fn wrap(inner: Arc<dyn ProviderUsageRecorder>) -> Arc<dyn ProviderUsageRecorder> {
        Arc::new(Self { inner })
    }
}

impl ProviderUsageRecorder for PrometheusProviderUsageRecorder {
    fn record_request(&self, event: ProviderUsageEvent) {
        let provider = event.provider_name.as_str();
        let model = event.model_name.as_deref().unwrap_or("unknown");
        let operation = sanitize_operation(&event.operation_name);
        let status = match event.status {
            ProviderCallStatus::Success => "success",
            ProviderCallStatus::Error => "error",
        };
        record_provider_request(provider, model, operation, status);
        if matches!(event.status, ProviderCallStatus::Error) {
            let class = if event.timed_out {
                "timeout"
            } else if event.circuit_blocked {
                "circuit_blocked"
            } else {
                "error"
            };
            record_provider_failure(provider, class);
        }
        record_provider_retries(provider, event.retry_count);
        if event.circuit_blocked {
            record_provider_circuit_open(provider);
        }
        self.inner.record_request(event);
    }

    fn snapshot(&self) -> ProviderUsageSnapshot {
        self.inner.snapshot()
    }
}

fn sanitize_operation(name: &str) -> &str {
    match name {
        "chat" | "embedding" | "extraction" | "summarization" | "rerank" => name,
        other if other.len() <= 32 => other,
        _ => "other",
    }
}
