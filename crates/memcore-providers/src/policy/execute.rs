use std::future::Future;
use std::time::Instant;

use memcore_common::{MemcoreError, MemcoreResult};

use super::retry::{backoff_duration, retry_decision_for, ProviderRetryDecision};
use super::timeout::provider_timeout_error;
use super::ProviderExecutionPolicy;

pub async fn execute_provider_call<F, Fut, T>(
    operation_name: &'static str,
    policy: &ProviderExecutionPolicy,
    mut operation: F,
) -> MemcoreResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = MemcoreResult<T>>,
{
    let max_attempts = policy.total_attempts();
    let timeout_ms = policy.timeout.as_millis();
    let mut last_error: Option<MemcoreError> = None;

    for attempt in 0..max_attempts {
        let attempt_number = attempt + 1;
        let started = Instant::now();

        match tokio::time::timeout(policy.timeout, operation()).await {
            Ok(Ok(result)) => {
                tracing::debug!(
                    operation_name = operation_name,
                    attempt_number = attempt_number,
                    max_attempts = max_attempts,
                    timeout_ms = timeout_ms,
                    success = true,
                    duration_ms = started.elapsed().as_millis(),
                    "provider call succeeded"
                );
                return Ok(result);
            }
            Ok(Err(error)) => {
                let retryable = matches!(
                    retry_decision_for(&error),
                    ProviderRetryDecision::Retry
                );
                tracing::warn!(
                    operation_name = operation_name,
                    attempt_number = attempt_number,
                    max_attempts = max_attempts,
                    timeout_ms = timeout_ms,
                    retryable = retryable,
                    success = false,
                    error_code = error.code(),
                    duration_ms = started.elapsed().as_millis(),
                    "provider call failed"
                );
                last_error = Some(error);

                if !retryable || attempt == policy.max_retries {
                    break;
                }

                let retry_number = attempt + 1;
                tokio::time::sleep(backoff_duration(policy, retry_number)).await;
            }
            Err(_elapsed) => {
                let error = provider_timeout_error();
                let retryable = matches!(
                    retry_decision_for(&error),
                    ProviderRetryDecision::Retry
                );
                tracing::warn!(
                    operation_name = operation_name,
                    attempt_number = attempt_number,
                    max_attempts = max_attempts,
                    timeout_ms = timeout_ms,
                    retryable = retryable,
                    success = false,
                    error_code = error.code(),
                    duration_ms = started.elapsed().as_millis(),
                    "provider call timed out"
                );
                last_error = Some(error);

                if !retryable || attempt == policy.max_retries {
                    break;
                }

                let retry_number = attempt + 1;
                tokio::time::sleep(backoff_duration(policy, retry_number)).await;
            }
        }
    }

    match last_error {
        Some(error) if error.is_provider_timeout() => Err(error),
        Some(error) if !super::retry::is_retryable_provider_error(&error) => Err(error),
        Some(error) => Err(MemcoreError::ProviderError(format!(
            "{operation_name} failed after {max_attempts} attempts: {}",
            error.message()
        ))),
        None => Err(MemcoreError::Internal(format!(
            "{operation_name} failed without an error"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use memcore_common::MemcoreError;

    use super::*;
    use crate::policy::ProviderExecutionPolicy;

    fn test_policy(max_retries: usize) -> ProviderExecutionPolicy {
        ProviderExecutionPolicy {
            max_retries,
            timeout: Duration::from_millis(200),
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(5),
            jitter_enabled: false,
            ..ProviderExecutionPolicy::default()
        }
    }

    #[tokio::test]
    async fn successful_operation_returns_immediately() {
        let policy = test_policy(2);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();

        let result = execute_provider_call("test_success", &policy, || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<i32, MemcoreError>(42)
            }
        })
        .await
        .expect("success");

        assert_eq!(result, 42);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn non_retryable_error_is_not_retried() {
        let policy = test_policy(2);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();

        let error = execute_provider_call("test_non_retryable", &policy, || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<i32, _>(MemcoreError::ValidationError("invalid payload".to_string()))
            }
        })
        .await
        .expect_err("should fail");

        assert!(matches!(error, MemcoreError::ValidationError(_)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retryable_error_is_retried_until_success() {
        let policy = test_policy(2);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();

        let result = execute_provider_call("test_retry_success", &policy, || {
            let calls = calls_for_closure.clone();
            async move {
                let attempt = calls.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt < 3 {
                    Err::<&str, _>(MemcoreError::ProviderError(
                        "OpenAI API error (503): unavailable".to_string(),
                    ))
                } else {
                    Ok("ok")
                }
            }
        })
        .await
        .expect("should eventually succeed");

        assert_eq!(result, "ok");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retries_exhausted_returns_final_error() {
        let policy = test_policy(2);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();

        let error = execute_provider_call("test_retry_exhausted", &policy, || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(MemcoreError::ProviderError(
                    "OpenAI API error (500): internal".to_string(),
                ))
            }
        })
        .await
        .expect_err("should fail");

        assert!(matches!(error, MemcoreError::ProviderError(_)));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn max_retries_zero_performs_only_one_attempt() {
        let policy = test_policy(0);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();

        let _ = execute_provider_call("test_no_retry", &policy, || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(MemcoreError::RateLimited)
            }
        })
        .await
        .expect_err("should fail");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn timeout_returns_provider_timeout_error() {
        let policy = ProviderExecutionPolicy {
            timeout: Duration::from_millis(20),
            max_retries: 0,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(1),
            jitter_enabled: false,
            ..ProviderExecutionPolicy::default()
        };

        let error = execute_provider_call("test_timeout", &policy, || async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(())
        })
        .await
        .expect_err("should time out");

        assert!(error.is_provider_timeout());
        assert_eq!(error.code(), "provider_timeout");
    }

    #[tokio::test]
    async fn timeout_is_retried_when_policy_allows() {
        let policy = ProviderExecutionPolicy {
            timeout: Duration::from_millis(30),
            max_retries: 1,
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(1),
            jitter_enabled: false,
            ..ProviderExecutionPolicy::default()
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();

        let result = execute_provider_call("test_timeout_retry", &policy, || {
            let calls = calls_for_closure.clone();
            async move {
                let attempt = calls.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt == 1 {
                    tokio::time::sleep(Duration::from_millis(80)).await;
                    Ok("late")
                } else {
                    Ok("fast")
                }
            }
        })
        .await
        .expect("second attempt should succeed");

        assert_eq!(result, "fast");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
