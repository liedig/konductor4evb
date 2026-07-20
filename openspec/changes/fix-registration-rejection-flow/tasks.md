# Tasks: fix-registration-rejection-flow

## 1. Re-registration After Rejection (register handler)

- [ ] 1.1 Widen the register conditional `PutItem` to `attribute_not_exists(domainName) OR #status = :REJECTED` in `domain-registrations/src/register.rs`
- [ ] 1.2 Preserve redelivery semantics: same-`requestId` conditional failure remains an idempotent no-op; different-`requestId` failure (now only non-REJECTED records) still emits `RegisterDomainFailed` "domain already exists"
- [ ] 1.3 Unit tests: re-register over REJECTED succeeds with a fresh record (no rejection fields carried over); duplicate over PENDING_APPROVAL/ACTIVE/PROVISIONING_FAILED still fails; same-requestId redelivery no-op

## 2. Registration Failure Notification

- [ ] 2.1 Scaffold `send-registration-failed-notification` crate following the existing notification Lambda pattern (event detail: `domainName`, `ownerEmail`, `reason`)
- [ ] 2.2 Add HTML email body builder with escaping + unit tests (reason, domain name, heading, HTML-escape of inputs)
- [ ] 2.3 Add the Lambda, log group, and EventBridge rule on `RegisterDomainFailed` to `nexus-core-stack.yaml` under the `NotificationsEnabled` condition
- [ ] 2.4 cfn-lint / `sam validate --lint`

## 3. Permanent SES Failure Classification

- [ ] 3.1 Add a shared classification helper in `nexus-common` (permanent: `MessageRejected`, `BadRequestException`; transient: everything else) with unit tests
- [ ] 3.2 Apply it in all four notification handlers: permanent → log at ERROR + acknowledge; transient → propagate (retry/DLQ unchanged)

## 4. Documentation

- [ ] 4.1 `docs/domain-management.md`: rejected registrations can be corrected and resubmitted under the same name; requester is emailed on deterministic failures
- [ ] 4.2 `docs/installation.md`: SES sandbox note — owner emails must be verified identities (or SES production access requested) for notifications to be delivered

## 5. Verify in Dev

- [ ] 5.1 Register a domain, reject it, re-register the same name with corrected details, approve, confirm it provisions to ACTIVE
- [ ] 5.2 Register a duplicate of an ACTIVE domain and confirm the failure email arrives with reason "domain already exists"
- [ ] 5.3 Trigger a send to an unverified address in sandbox and confirm: single ERROR log, no retries, nothing in the Lambda failure DLQ
