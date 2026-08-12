# Proposal: fix-registration-rejection-flow

## Why

A rejected domain registration is a dead end: the rejected record stays in `konductor-domains` and the register handler's `attribute_not_exists(domainName)` condition permanently blocks the name, while updates are only allowed on ACTIVE domains. Worse, the resulting `RegisterDomainFailed` event is consumed by nothing, so the requester gets a `202 Accepted` and then silence. Both were hit during live verification: a registration with a wrong account id was rejected, and the corrected re-registration silently did nothing.

## What Changes

- **Re-registration over a rejected record**: the register handler's conditional `PutItem` becomes `attribute_not_exists(domainName) OR #status = :REJECTED`, so a fresh registration replaces a rejected one (new `requestId`, fresh approval cycle). The rejection audit trail lives in the event stream on `konductor-bus`; the record is replaced wholesale. Mirrors the subscriptions precedent (re-subscribe after removal gets a fresh contract).
- **Registration failure notification**: a new EventBridge rule on `RegisterDomainFailed` invokes a new `send-registration-failed-notification` Lambda that emails the submitted `ownerEmail` with the failure reason (same conditional-deployment pattern as the existing three notification Lambdas).
- **Permanent email failures are not retried**: notification Lambdas treat deterministic SES rejections (unverified identity in sandbox, malformed address) as permanent, log them, and acknowledge instead of retrying into the Lambda failure DLQ.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `domain-management`: the Register Domain uniqueness rule changes from "name never reusable once a record exists" to "name reusable when the existing record is `REJECTED`".
- `domain-owner-notifications`: adds a registration-failure notification requirement and a permanent-failure (no-retry) rule for notification sends.

## Impact

- `domain-management/functions/domain-registrations/src/register.rs`: condition expression, replaced-record handling, tests.
- New crate `domain-management/functions/send-registration-failed-notification` + EventBridge rule and conditional resources in `konductor-core-stack.yaml`.
- Existing notification handlers: classify SES `MessageRejected`/`BadRequestException` as permanent (log, acknowledge).
- Docs: `docs/domain-management.md` (re-registration after rejection), `docs/installation.md` (SES sandbox note: owner emails must be verified identities or SES must be in production mode).
