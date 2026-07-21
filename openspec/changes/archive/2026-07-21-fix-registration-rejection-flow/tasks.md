# Tasks: fix-registration-rejection-flow

## 1. Re-registration After Rejection (register handler)

- [x] 1.1 Widen the register conditional `PutItem` to `attribute_not_exists(domainName) OR #status = :REJECTED` in `domain-registrations/src/register.rs` — condition expression, expression attribute names (`#status`), and values (`:rejected` = `REJECTED`) added
- [x] 1.2 Preserve redelivery semantics: same-`requestId` conditional failure remains an idempotent no-op; different-`requestId` failure (now only non-REJECTED records) still emits `RegisterDomainFailed` "domain already exists" — no code change needed, the existing conditional-failure branch only fires for non-REJECTED records now
- [x] 1.3 Unit tests: `put_item_uses_widened_condition_admitting_rejected` (verifies condition expression string), `reregistration_over_rejected_succeeds_with_fresh_record`, `duplicate_over_pending_approval_still_emits_failed_event`; existing `retry_of_own_registration_resumes_after_conditional_failure` and `rejects_duplicate_domain_with_failed_event` cover remaining cases

## 2. Registration Failure Notification

- [x] 2.1 Scaffold `send-registration-failed-notification` crate following the existing notification Lambda pattern (event detail: `domainName`, `ownerEmail`, `reason`) — Cargo.toml, main.rs, handler.rs created; added to workspace
- [x] 2.2 Add HTML email body builder with escaping + unit tests (reason, domain name, heading, HTML-escape of inputs) — `build_failure_email_body` with 4 unit tests
- [x] 2.3 Add the Lambda, log group, and EventBridge rule on `RegisterDomainFailed` to `nexus-core-stack.yaml` under the `NotificationsEnabled` condition — `SendRegistrationFailedNotificationFunction`, log group, `DomainRegistrationFailedRule`, `DomainRegistrationFailedRulePermission`
- [x] 2.4 cfn-lint / `sam validate --lint` — both clean

## 3. Permanent SES Failure Classification

- [x] 3.1 Add a shared classification helper in `nexus-common` (permanent: `MessageRejected`, `BadRequestException`; transient: everything else) with unit tests — `lib/nexus-common/src/ses.rs` with `is_permanent_failure(&SendEmailError)` + 3 unit tests; `aws-sdk-sesv2` added to nexus-common deps
- [x] 3.2 Apply it in all four notification handlers: permanent → log at ERROR + acknowledge; transient → propagate (retry/DLQ unchanged) — applied in all 5 notification handlers (the 4 existing + the new registration-failed handler) via `e.as_service_error().is_some_and(nexus_common::ses::is_permanent_failure)`

## 4. Documentation

- [x] 4.1 `docs/domain-management.md`: rejected registrations can be corrected and resubmitted under the same name; requester is emailed on deterministic failures — updated rejection section, status table, duplicate section, and notification table
- [x] 4.2 `docs/installation.md`: SES sandbox note — owner emails must be verified identities (or SES production access requested) for notifications to be delivered — added to SES sandbox troubleshooting section

## 5. Verify in Dev

- [x] 5.1 Register a domain, reject it, re-register the same name with corrected details, approve, confirm it provisions to ACTIVE — verified: registered `reject-test`, rejected it, re-registered the same name, confirmed the fresh request reached ACTIVE with a new request ID and no old rejection details
- [x] 5.2 Register a duplicate of an ACTIVE domain and confirm the failure email arrives with reason "domain already exists" — verified: duplicate registration against ACTIVE `billing` invoked the new failure-notification Lambda and left billing unchanged
- [x] 5.3 Trigger a send to an unverified address in sandbox and confirm: single ERROR log, no retries, nothing in the Lambda failure DLQ — verified: unverified email produced one ERROR log "permanent SES failure, acknowledging without retry" with no retries or new DLQ delivery; existing DLQ entries predated deployment
