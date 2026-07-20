# Design: fix-registration-rejection-flow

## Context

Registration is async by design (202 + SQS + events, decision D8). The register handler enforces name uniqueness with a conditional `PutItem` (`attribute_not_exists(domainName)`), tolerating only same-`requestId` redeliveries. Rejection keeps the record (`status=REJECTED`, `decidedBy`, `justification`, `rejectedAt`). Three notification Lambdas exist (approval request, approval outcome, rejection outcome), deployed conditionally when SES parameters are set; async-invoke failures retry twice then land in the Lambda failure DLQ.

Observed gaps: a rejected name cannot be re-registered; `RegisterDomainFailed` has no consumer, so deterministic failures are invisible to the requester; SES sandbox rejections (unverified recipient) burn retries and land in the DLQ even though they can never succeed.

## Goals / Non-Goals

**Goals:**

- A rejected registration can be corrected and resubmitted under the same name without operator intervention.
- Every deterministic registration failure produces an email to the submitted `ownerEmail`.
- Deterministic SES failures are logged once and acknowledged, not retried.

**Non-Goals:**

- Pre-approval editing of a `PENDING_APPROVAL` registration (the flow is reject, then re-register).
- Re-registration over `ACTIVE`, `PROVISIONING`, or `PROVISIONING_FAILED` records (failed provisioning has partially-known state; teardown/deletion flows own that).
- Synchronous validation at the API (would reverse decision D8).
- SES production-access automation; the sandbox constraint is documented, not solved.

## Decisions

**D1: Widen the PutItem condition rather than delete-then-put.**
`attribute_not_exists(domainName) OR #s = :rejected` keeps the uniqueness check atomic (no read-then-write race between two concurrent registrations of a rejected name). The replacement writes a complete fresh record, dropping `decidedBy`/`justification`/`rejectedAt`; the event stream on `nexus-bus` is the audit trail. Alternative (copy prior rejection into a `previousRejection` attribute) rejected: duplicates what events already record and complicates the item model.

**D2: Same-`requestId` redelivery tolerance is preserved.**
The conditional-check-failure branch still distinguishes: same `requestId` = idempotent redelivery (no-op success); different `requestId` + non-REJECTED status = genuine duplicate (emit `RegisterDomainFailed`). The REJECTED case never reaches the failure branch because the condition now admits it.

**D3: Failure notification follows the existing notification pattern exactly.**
New Lambda + EventBridge rule on `source: nexus.domain-management, detail-type: RegisterDomainFailed`, gated by the same `NotificationsEnabled` condition, emailing `detail.ownerEmail` with domain name and reason. Alternative (extend an existing Lambda to multiplex detail-types) rejected: the codebase convention is one narrow Lambda per notification.

**D4: Permanent SES failures are classified in the shared layer.**
SES `MessageRejected` and `BadRequestException` are deterministic for a given recipient; retrying cannot succeed. Handlers log at ERROR with the failure class and return `Ok`, acknowledging the invoke. Transient SES errors (throttling, 5xx) keep the current behavior (error propagates, async retry, DLQ). Classification lives in one helper in `nexus-common` so all four notification Lambdas share it.

## Risks / Trade-offs

- [Replacing a rejected record erases its item-level audit fields] → the rejection emitted `DomainRegistrationRejected` with `decidedBy`/`justification`; event history is the system of record.
- [Failure emails go to an unauthenticated, requester-submitted address] → registration is IAM-authenticated and the email only echoes the requester's own submission back to them; no new information disclosure.
- [Permanent-failure classification could mask a misconfigured sender identity] → the ERROR log carries the SES error verbatim; the alarm posture on notification failures is unchanged for transient errors.

## Migration Plan

1. Ship `nexus-common` classification helper + handler updates + new Lambda + template rule in one core-stack deploy.
2. No data migration: existing REJECTED records become re-registerable retroactively by virtue of the new condition.
3. Rollback: redeploy the previous stack; rejected names become blocked again (no data loss).

## Open Questions

(none)
