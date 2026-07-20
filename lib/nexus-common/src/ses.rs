//! SES send-error classification for notification Lambdas.
//!
//! Notification Lambdas share one classification rule: deterministic SES
//! rejections (unverified identity in sandbox mode, malformed address) are
//! permanent and must not be retried, because retrying a deterministic
//! rejection cannot succeed. Transient failures (throttling, service errors)
//! propagate so the asynchronous invocation retry and failure-DLQ behavior
//! applies unchanged.

use aws_sdk_sesv2::operation::send_email::SendEmailError;

/// Returns `true` when the SES `SendEmail` service error is permanent —
/// retrying the same request with the same parameters cannot succeed. The
/// two permanent cases are:
///
/// - `MessageRejected` — SES refused the message (e.g. the recipient identity
///   is not verified while the account is in the sandbox).
/// - `BadRequestException` — the request is structurally invalid (e.g. a
///   malformed email address that SES normalisation cannot repair).
///
/// All other service errors (throttling, internal service error, etc.) are
/// transient. Callers should extract the service error from the `SdkError`
/// via `err.service_error()` before calling this helper; a `None` service
/// error (network, timeout) is transient by definition.
pub fn is_permanent_failure(err: &SendEmailError) -> bool {
    matches!(
        err,
        SendEmailError::MessageRejected(_) | SendEmailError::BadRequestException(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_sesv2::types::error::{BadRequestException, MessageRejected};

    #[test]
    fn message_rejected_is_permanent() {
        let err = SendEmailError::MessageRejected(MessageRejected::builder().build());
        assert!(is_permanent_failure(&err));
    }

    #[test]
    fn bad_request_is_permanent() {
        let err = SendEmailError::BadRequestException(BadRequestException::builder().build());
        assert!(is_permanent_failure(&err));
    }

    #[test]
    fn other_service_error_is_transient() {
        let err = SendEmailError::unhandled("internal service error");
        assert!(!is_permanent_failure(&err));
    }
}
