use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use memcore_common::{MemcoreError, MemcoreResult};

use crate::circuit_breaker::{CircuitState, ProviderCircuitBreaker};
use crate::policy::{execute_provider_call, is_provider_health_failure, ProviderExecutionPolicy};

use super::metrics::ProviderRoutingMetrics;
use super::types::{circuit_key, ProviderCapability, ProviderId};

#[derive(Debug, Clone)]
pub struct ProviderCandidate<P> {
    pub provider_id: ProviderId,
    pub provider: P,
}

pub struct ProviderFallbackRouter {
    circuit_breaker: Arc<ProviderCircuitBreaker>,
    policy: ProviderExecutionPolicy,
    metrics: Option<Arc<ProviderRoutingMetrics>>,
}

impl ProviderFallbackRouter {
    pub fn new(
        circuit_breaker: Arc<ProviderCircuitBreaker>,
        policy: ProviderExecutionPolicy,
        metrics: Option<Arc<ProviderRoutingMetrics>>,
    ) -> Self {
        Self {
            circuit_breaker,
            policy,
            metrics,
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
        F: FnMut(P) -> Fut,
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

        let mut last_error: Option<MemcoreError> = None;
        let mut attempted_fallback = false;

        for (index, candidate) in providers_to_try.iter().enumerate() {
            let provider_id = ProviderId::new(candidate.provider_id.name.clone(), capability);
            let key = circuit_key(&provider_id, operation_name);

            if let Err(error) = self.circuit_breaker.check_allow(&key) {
                if let Some(metrics) = &self.metrics {
                    metrics.record_circuit_blocked();
                }
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
            let result = execute_provider_call(operation_name, &self.policy, || {
                call(candidate.provider.clone())
            })
            .await;

            match result {
                Ok(value) => {
                    self.circuit_breaker.record_success(&key);
                    if let Some(metrics) = &self.metrics {
                        metrics.record_call_success();
                        if index > 0 {
                            metrics.record_fallback_succeeded();
                        }
                    }
                    tracing::debug!(
                        operation_name = operation_name,
                        capability = %capability,
                        provider_name = %provider_id.name,
                        attempt_provider_index = index,
                        fallback_enabled = fallback_enabled,
                        fallback_used = index > 0,
                        circuit_state = ?self.circuit_breaker.snapshot(&key).state,
                        circuit_open = false,
                        success = true,
                        duration_ms = started.elapsed().as_millis(),
                        "provider call succeeded"
                    );
                    return Ok(value);
                }
                Err(error) => {
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

                    tracing::warn!(
                        operation_name = operation_name,
                        capability = %capability,
                        provider_name = %provider_id.name,
                        attempt_provider_index = index,
                        fallback_enabled = fallback_enabled,
                        fallback_used = index > 0,
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
    use crate::{circuit_key, ProviderId};

    fn test_router() -> ProviderFallbackRouter {
        ProviderFallbackRouter::new(
            Arc::new(ProviderCircuitBreaker::new(CircuitBreakerConfig::for_tests())),
            ProviderExecutionPolicy::for_tests(),
            Some(ProviderRoutingMetrics::new()),
        )
    }

    fn candidate(name: &str) -> ProviderCandidate<Arc<AtomicUsize>> {
        ProviderCandidate {
            provider_id: ProviderId::new(name, ProviderCapability::Llm),
            provider: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[tokio::test]
    async fn primary_success_does_not_call_fallback() {
        let router = test_router();
        let primary = candidate("primary");
        let fallback = candidate("fallback");
        let primary_counter = primary.provider.clone();

        let result = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                true,
                &[primary, fallback],
                |provider| async move {
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
        let router = test_router();
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
                |provider| async move {
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
        let router = test_router();
        let primary = candidate("primary");
        let fallback = candidate("fallback");
        let fallback_counter = fallback.provider.clone();

        let error = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                true,
                &[primary, fallback],
                |_provider| async move {
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
        let router = ProviderFallbackRouter::new(
            breaker.clone(),
            ProviderExecutionPolicy {
                max_retries: 0,
                ..ProviderExecutionPolicy::for_tests()
            },
            Some(ProviderRoutingMetrics::new()),
        );
        let primary = ProviderCandidate {
            provider_id: ProviderId::new("primary", ProviderCapability::Llm),
            provider: Arc::new(AtomicUsize::new(0)),
        };
        let fallback = ProviderCandidate {
            provider_id: ProviderId::new("fallback", ProviderCapability::Llm),
            provider: Arc::new(AtomicUsize::new(0)),
        };
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
                |provider| async move {
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
    }

    #[tokio::test]
    async fn fallback_disabled_does_not_call_secondary_provider() {
        let router = test_router();
        let primary = candidate("primary");
        let fallback = candidate("fallback");
        let fallback_counter = fallback.provider.clone();

        let _ = router
            .execute_with_fallback(
                ProviderCapability::Llm,
                "test_op",
                false,
                &[primary, fallback],
                |_provider| async move {
                    Err::<(), _>(MemcoreError::ProviderError(
                        "OpenAI API error (500): internal".to_string(),
                    ))
                },
            )
            .await
            .expect_err("fail");

        assert_eq!(fallback_counter.load(Ordering::SeqCst), 0);
    }
}
