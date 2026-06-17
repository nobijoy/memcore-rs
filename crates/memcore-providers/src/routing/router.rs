use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use memcore_common::{MemcoreError, MemcoreResult};

use crate::circuit_breaker::{CircuitState, ProviderCircuitBreaker};
use crate::policy::{execute_provider_call, is_provider_health_failure, ProviderExecutionPolicy};
use crate::usage::{
    estimate_event_cost, take_token_usage, ProviderCallStatus, ProviderUsageCapability,
    ProviderUsageEvent, ProviderUsageRecorder, TokenUsageSlot,
};

use super::metrics::ProviderRoutingMetrics;
use super::types::{circuit_key, ProviderCapability, ProviderId};

#[derive(Debug, Clone)]
pub struct ProviderCandidate<P> {
    pub provider_id: ProviderId,
    pub provider: P,
    pub model_name: Option<String>,
    pub token_usage_slot: Option<TokenUsageSlot>,
}

impl<P> ProviderCandidate<P> {
    pub fn new(
        provider_id: ProviderId,
        provider: P,
        model_name: Option<String>,
        token_usage_slot: Option<TokenUsageSlot>,
    ) -> Self {
        Self {
            provider_id,
            provider,
            model_name,
            token_usage_slot,
        }
    }
}

pub struct ProviderFallbackRouter {
    circuit_breaker: Arc<ProviderCircuitBreaker>,
    policy: ProviderExecutionPolicy,
    metrics: Option<Arc<ProviderRoutingMetrics>>,
    usage_recorder: Option<Arc<dyn ProviderUsageRecorder>>,
    cost_tracking_enabled: bool,
}

impl ProviderFallbackRouter {
    pub fn new(
        circuit_breaker: Arc<ProviderCircuitBreaker>,
        policy: ProviderExecutionPolicy,
        metrics: Option<Arc<ProviderRoutingMetrics>>,
        usage_recorder: Option<Arc<dyn ProviderUsageRecorder>>,
        cost_tracking_enabled: bool,
    ) -> Self {
        Self {
            circuit_breaker,
            policy,
            metrics,
            usage_recorder,
            cost_tracking_enabled,
        }
    }

    fn record_usage(
        &self,
        capability: ProviderUsageCapability,
        operation_name: &str,
        provider_name: &str,
        model_name: Option<&str>,
        status: ProviderCallStatus,
        token_usage_slot: Option<&TokenUsageSlot>,
        retry_count: u64,
        fallback_used: bool,
        circuit_blocked: bool,
        timed_out: bool,
    ) {
        let Some(recorder) = &self.usage_recorder else {
            return;
        };

        let token_usage = token_usage_slot.and_then(take_token_usage);
        let input_tokens = token_usage.and_then(|usage| usage.input_tokens);
        let output_tokens = token_usage.and_then(|usage| usage.output_tokens);
        let estimated_cost_usd = estimate_event_cost(
            self.cost_tracking_enabled,
            provider_name,
            model_name,
            capability,
            input_tokens,
            output_tokens,
        );

        recorder.record_request(ProviderUsageEvent {
            provider_name: provider_name.to_string(),
            model_name: model_name.map(str::to_string),
            capability,
            operation_name: operation_name.to_string(),
            status,
            input_tokens,
            output_tokens,
            retry_count,
            fallback_used,
            circuit_blocked,
            timed_out,
            estimated_cost_usd,
        });
    }

    fn usage_capability(capability: ProviderCapability) -> ProviderUsageCapability {
        match capability {
            ProviderCapability::Llm => ProviderUsageCapability::Llm,
            ProviderCapability::Embedding => ProviderUsageCapability::Embedding,
            ProviderCapability::Summarization => ProviderUsageCapability::Summarization,
        }
    }

    pub async fn execute_with_fallback<P, F, Fut, T>(
        &self,
        capability: ProviderCapability,
        operation_name: &'static str,
        fallback_enabled: bool,
        candidates: &[ProviderCandidate<P>],
        mut call: F,
    ) -> MemcoreResult<T>
    where
        P: Clone,
        F: FnMut(P, Option<TokenUsageSlot>) -> Fut,
        Fut: Future<Output = MemcoreResult<T>>,
    {
        if candidates.is_empty() {
            return Err(MemcoreError::Internal(
                "no provider candidates configured".to_string(),
            ));
        }

        let providers_to_try = if fallback_enabled {
            candidates
        } else {
            &candidates[..1]
        };

        let usage_capability = Self::usage_capability(capability);
        let mut last_error: Option<MemcoreError> = None;
        let mut attempted_fallback = false;

        for (index, candidate) in providers_to_try.iter().enumerate() {
            let provider_id = ProviderId::new(candidate.provider_id.name.clone(), capability);
            let key = circuit_key(&provider_id, operation_name);
            let fallback_used = index > 0;
            let model_name = candidate.model_name.as_deref();

            if let Err(error) = self.circuit_breaker.check_allow(&key) {
                if let Some(metrics) = &self.metrics {
                    metrics.record_circuit_blocked();
                }
                self.record_usage(
                    usage_capability,
                    operation_name,
                    &provider_id.name,
                    model_name,
                    ProviderCallStatus::Error,
                    candidate.token_usage_slot.as_ref(),
                    0,
                    fallback_used,
                    true,
                    false,
                );
                tracing::warn!(
                    operation_name = operation_name,
                    capability = %capability,
                    provider_name = %provider_id.name,
                    attempt_provider_index = index,
                    fallback_enabled = fallback_enabled,
                    circuit_state = ?self.circuit_breaker.snapshot(&key).state,
                    circuit_open = true,
                    error_code = error.code(),
                    "provider call blocked by open circuit"
                );
                last_error = Some(error);
                if !fallback_enabled || index == providers_to_try.len() - 1 {
                    break;
                }
                if index == 0 {
                    attempted_fallback = true;
                    if let Some(metrics) = &self.metrics {
                        metrics.record_fallback_attempted();
                    }
                }
                continue;
            }

            if self.circuit_breaker.snapshot(&key).state == CircuitState::HalfOpen {
                if let Some(metrics) = &self.metrics {
                    metrics.record_circuit_half_opened();
                }
            }

            let started = Instant::now();
            let token_slot = candidate.token_usage_slot.clone();
            let result = execute_provider_call(operation_name, &self.policy, || {
                call(
                    candidate.provider.clone(),
                    token_slot.clone(),
                )
            })
            .await;

            match result {
                Ok(outcome) => {
                    self.circuit_breaker.record_success(&key);
                    if let Some(metrics) = &self.metrics {
                        metrics.record_call_success();
                        if index > 0 {
                            metrics.record_fallback_succeeded();
                        }
                    }
                    self.record_usage(
                        usage_capability,
                        operation_name,
                        &provider_id.name,
                        model_name,
                        ProviderCallStatus::Success,
                        candidate.token_usage_slot.as_ref(),
                        outcome.retries as u64,
                        fallback_used,
                        false,
                        outcome.timed_out,
                    );
                    tracing::debug!(
                        operation_name = operation_name,
                        capability = %capability,
                        provider_name = %provider_id.name,
                        attempt_provider_index = index,
                        fallback_enabled = fallback_enabled,
                        fallback_used = fallback_used,
                        circuit_state = ?self.circuit_breaker.snapshot(&key).state,
                        circuit_open = false,
                        success = true,
                        retry_count = outcome.retries,
                        duration_ms = started.elapsed().as_millis(),
                        "provider call succeeded"
                    );
                    return Ok(outcome.value);
                }
                Err(failure) => {
                    let error = failure.error;
                    if let Some(metrics) = &self.metrics {
                        metrics.record_call_failure();
                    }

                    let retryable_failure = is_provider_health_failure(&error);
                    if retryable_failure {
                        let before = self.circuit_breaker.snapshot(&key);
                        self.circuit_breaker.record_failure(&key);
                        let after = self.circuit_breaker.snapshot(&key);
                        if before.state != after.state && after.state == CircuitState::Open {
                            if let Some(metrics) = &self.metrics {
                                metrics.record_circuit_opened();
                            }
                        }
                    }

                    self.record_usage(
                        usage_capability,
                        operation_name,
                        &provider_id.name,
                        model_name,
                        ProviderCallStatus::Error,
                        candidate.token_usage_slot.as_ref(),
                        failure.retries as u64,
                        fallback_used,
                        false,
                        failure.timed_out,
                    );

                    tracing::warn!(
                        operation_name = operation_name,
                        capability = %capability,
                        provider_name = %provider_id.name,
                        attempt_provider_index = index,
                        fallback_enabled = fallback_enabled,
                        fallback_used = fallback_used,
                        circuit_state = ?self.circuit_breaker.snapshot(&key).state,
                        circuit_open = error.is_provider_circuit_open(),
                        success = false,
                        duration_ms = started.elapsed().as_millis(),
                        error_code = error.code(),
                        retryable_failure = retryable_failure,
                        "provider call failed"
                    );

                    if !retryable_failure {
                        return Err(error);
                    }

                    last_error = Some(error);
                    if !fallback_enabled || index == providers_to_try.len() - 1 {
                        break;
                    }
                    if index == 0 && !attempted_fallback {
                        attempted_fallback = true;
                        if let Some(metrics) = &self.metrics {
                            metrics.record_fallback_attempted();
                        }
                    }
                }
            }
        }

        match last_error {
            Some(error) if error.is_provider_circuit_open() => Err(error),
            Some(error) if !is_provider_health_failure(&error) => Err(error),
            Some(error) => Err(MemcoreError::ProviderError(format!(
                "{operation_name} failed for all configured providers: {}",
                error.message()
            ))),
            None => Err(MemcoreError::ProviderError(format!(
                "{operation_name} failed: all provider circuits are open"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use memcore_common::MemcoreError;

    use super::*;
    use crate::circuit_breaker::CircuitBreakerConfig;
    use crate::policy::ProviderExecutionPolicy;
    use crate::usage::InMemoryProviderUsageRecorder;
    use crate::{circuit_key, ProviderId};

    fn test_router(usage: Option<Arc<dyn ProviderUsageRecorder>>) -> ProviderFallbackRouter {
        ProviderFallbackRouter::new(
            Arc::new(ProviderCircuitBreaker::new(CircuitBreakerConfig::for_tests())),
            ProviderExecutionPolicy::for_tests(),
            Some(ProviderRoutingMetrics::new()),
            usage,
            false,
        )
    }

    fn candidate(name: &str) -> ProviderCandidate<Arc<AtomicUsize>> {
        ProviderCandidate::new(
            ProviderId::new(name, ProviderCapability::Llm),
            Arc::new(AtomicUsize::new(0)),
            Some(format!("{name}-model")),
            None,
        )
    }

    #[tokio::test]
    async fn primary_success_does_not_call_fallback() {
        let router = test_router(None);
        let primary = candidate("primary");
        let fallback = candidate("fallback");
        let primary_counter = primary.provider.clone();

        let result = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                true,
                &[primary, fallback],
                |provider, _slot| async move {
                    provider.fetch_add(1, Ordering::SeqCst);
                    Ok(7)
                },
            )
            .await
            .expect("success");

        assert_eq!(result, 7);
        assert_eq!(primary_counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retryable_primary_failure_calls_fallback() {
        let router = test_router(None);
        let primary = candidate("primary");
        let fallback = candidate("fallback");
        let primary_ptr = Arc::as_ptr(&primary.provider);
        let fallback_counter = fallback.provider.clone();

        let result = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                true,
                &[primary, fallback],
                |provider, _slot| async move {
                    if Arc::as_ptr(&provider) == primary_ptr {
                        Err(MemcoreError::ProviderError(
                            "OpenAI API error (503): unavailable".to_string(),
                        ))
                    } else {
                        provider.fetch_add(1, Ordering::SeqCst);
                        Ok(11)
                    }
                },
            )
            .await
            .expect("fallback success");

        assert_eq!(result, 11);
        assert_eq!(fallback_counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn non_retryable_primary_error_does_not_call_fallback() {
        let router = test_router(None);
        let primary = candidate("primary");
        let fallback = candidate("fallback");
        let fallback_counter = fallback.provider.clone();

        let error = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                true,
                &[primary, fallback],
                |_provider, _slot| async move {
                    Err::<i32, _>(MemcoreError::ValidationError("bad".to_string()))
                },
            )
            .await
            .expect_err("validation");

        assert!(matches!(error, MemcoreError::ValidationError(_)));
        assert_eq!(fallback_counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn open_circuit_skips_provider_and_uses_fallback() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let breaker = Arc::new(ProviderCircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            reset_timeout: std::time::Duration::from_secs(3600),
            half_open_max_calls: 1,
            enabled: true,
        }));
        let usage = InMemoryProviderUsageRecorder::new();
        let router = ProviderFallbackRouter::new(
            breaker.clone(),
            ProviderExecutionPolicy {
                max_retries: 0,
                ..ProviderExecutionPolicy::for_tests()
            },
            Some(ProviderRoutingMetrics::new()),
            Some(usage.clone()),
            false,
        );
        let primary = ProviderCandidate::new(
            ProviderId::new("primary", ProviderCapability::Llm),
            Arc::new(AtomicUsize::new(0)),
            Some("primary-model".to_string()),
            None,
        );
        let fallback = ProviderCandidate::new(
            ProviderId::new("fallback", ProviderCapability::Llm),
            Arc::new(AtomicUsize::new(0)),
            Some("fallback-model".to_string()),
            None,
        );
        let primary_ptr = Arc::as_ptr(&primary.provider);
        let fallback_counter = fallback.provider.clone();
        let key = circuit_key(&ProviderId::new("primary", ProviderCapability::Llm), "test_op");
        breaker.record_failure(&key);

        let result = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                true,
                &[primary, fallback],
                |provider, _slot| async move {
                    provider.fetch_add(1, Ordering::SeqCst);
                    if Arc::as_ptr(&provider) == primary_ptr {
                        Ok(1)
                    } else {
                        Ok(2)
                    }
                },
            )
            .await
            .expect("fallback should run");

        assert_eq!(result, 2);
        assert_eq!(fallback_counter.load(Ordering::SeqCst), 1);
        let snapshot = usage.snapshot();
        assert!(snapshot.total_circuit_blocks >= 1);
    }

    #[tokio::test]
    async fn fallback_disabled_does_not_call_secondary_provider() {
        let router = test_router(None);
        let primary = candidate("primary");
        let fallback = candidate("fallback");
        let fallback_counter = fallback.provider.clone();

        let _ = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                false,
                &[primary, fallback],
                |_provider, _slot| async move {
                    Err::<(), _>(MemcoreError::ProviderError(
                        "OpenAI API error (500): internal".to_string(),
                    ))
                },
            )
            .await
            .expect_err("fail");

        assert_eq!(fallback_counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn usage_recorder_records_success_and_fallback() {
        let usage = InMemoryProviderUsageRecorder::new();
        let router = test_router(Some(usage.clone()));
        let primary = candidate("primary");
        let fallback = candidate("fallback");
        let primary_ptr = Arc::as_ptr(&primary.provider);

        let _ = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                true,
                &[primary, fallback],
                |provider, _slot| async move {
                    if Arc::as_ptr(&provider) == primary_ptr {
                        Err(MemcoreError::ProviderError(
                            "OpenAI API error (503): unavailable".to_string(),
                        ))
                    } else {
                        Ok(1)
                    }
                },
            )
            .await
            .expect("fallback");

        let snapshot = usage.snapshot();
        assert_eq!(snapshot.total_requests, 2);
        assert_eq!(snapshot.total_successes, 1);
        assert_eq!(snapshot.total_errors, 1);
        assert!(snapshot.total_fallbacks >= 1);
    }

    #[tokio::test]
    async fn usage_recorder_records_non_retryable_without_fallback() {
        let usage = InMemoryProviderUsageRecorder::new();
        let router = test_router(Some(usage.clone()));
        let primary = candidate("primary");
        let fallback = candidate("fallback");

        let _ = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                true,
                &[primary, fallback],
                |_provider, _slot| async move {
                    Err::<(), _>(MemcoreError::ValidationError("bad".to_string()))
                },
            )
            .await
            .expect_err("validation");

        let snapshot = usage.snapshot();
        assert_eq!(snapshot.total_requests, 1);
        assert_eq!(snapshot.total_errors, 1);
        assert_eq!(snapshot.total_fallbacks, 0);
    }
}
