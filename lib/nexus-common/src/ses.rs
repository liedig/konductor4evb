//! SES error-code classification for notification Lambdas.
//!
//! Notification Lambdas share one classification rule: deterministic SES
//! rejections (unverified identity in sandbox mode, malformed address) are
//! permanent and must not be retried, because retrying a deterministic
//! rejection cannot succeed. Transient failures (throttling, service errors)
//! propagate so the asynchronous invocation retry and failure-DLQ behavior
//! applies unchanged.

/// Returns `true` when the SES `SendEmail` error code is permanent.
///
/// All other service errors and errors without service metadata are transient.
pub fn is_permanent_failure(error_code: &str) -> bool {
    matches!(error_code, "MessageRejected" | "BadRequestException")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_rejected_is_permanent() {
        assert!(is_permanent_failure("MessageRejected"));
    }

    #[test]
    fn bad_request_is_permanent() {
        assert!(is_permanent_failure("BadRequestException"));
    }

    #[test]
    fn other_service_error_is_transient() {
        assert!(!is_permanent_failure("InternalServiceError"));
    }
}
