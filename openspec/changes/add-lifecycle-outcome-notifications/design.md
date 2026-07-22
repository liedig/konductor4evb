# Design: add-lifecycle-outcome-notifications

## Context

Lifecycle events on the nexus bus drive notifications through a consistent pattern: one narrow Lambda per detail-type, an EventBridge rule gated by the `NotificationsEnabled` condition, SES for delivery, DLQs on both the rule target and the Lambda. Two events have no consumers (`DomainProvisioningSucceeded`, `DomainApprovalNotificationFailed`), owner-facing notifications ignore `secondaryOwnerEmails`, and platform operators receive approval requests but no lifecycle outcomes at all.

`PlatformOperatorEmail` already exists as a core-stack parameter and is SES-verified by the notifications stack. Recipient resolution today uses two styles: from the event detail (approval outcome, rejection outcome, registration failed) and from a DynamoDB lookup by `domainName` (provisioning failed).

## Goals / Non-Goals

**Goals:**
- Operators receive every lifecycle event on the nexus bus through a single subscription point.
- Owners learn when their domain goes live, and when their registration is delayed by a notification-infrastructure failure.
- Secondary owners receive every owner-facing notification.
- Leave a clean extensibility seam for future notification channels without committing to any adapter now.

**Non-Goals:**
- Channel adapters (Slack, Outlook/SMTP, webhooks) — future changes, they become SNS topic subscribers.
- Per-domain owner channel preferences — a domain-model change, out of scope.
- HTML formatting or per-event templating for operator messages — plain readable payloads suffice while the email channel is being simulated.

## Decisions

### D1: Operator channel is an SNS topic fed by one EventBridge rule — no Lambda

A `nexus-platform-notifications` SNS topic with an email subscription for `PlatformOperatorEmail`. One rule on the nexus bus with pattern `{"source": [{"prefix": "nexus."}]}` targets the topic, covering `nexus.domain-management` and `nexus.event-streams` events alike, and any future `nexus.*` source, with zero code. A topic policy grants `events.amazonaws.com` publish scoped to the rule ARN. The rule target carries a `DeadLetterConfig` on the existing `EventBridgeRuleFailureDlq`.

Alternative rejected: per-event operator Lambdas formatting SES emails — more code and SES coupling for an audience that needs signal, not polish. SNS also decouples the operator path from SES entirely, which matters for `DomainApprovalNotificationFailed` (the event that fires precisely when SES delivery failed).

### D2: Input transformer produces a compact JSON message, not prose

The rule uses an `InputTransformer` with `InputPathsMap` capturing `detail-type`, `source`, `time`, and `$.detail`, rendering `{"event": <detailType>, "source": <source>, "time": <time>, "detail": <detail>}`. Rationale: every captured path is guaranteed present on every EventBridge event (no per-detail-type path that could fail transformation on heterogeneous events), and the full detail payload is preserved for diagnosis. EventBridge cannot set a per-message SNS email subject; the topic `DisplayName` (`Nexus Platform`) identifies the sender instead.

Alternative rejected: prose template with per-event fields — any `InputPathsMap` path missing from a given event fails delivery for that event; the union of fields across all lifecycle events shares only the envelope.

### D3: Both new owner Lambdas resolve recipients via DynamoDB lookup

`send-provisioning-succeeded-notification` (on `DomainProvisioningSucceeded`) and `send-registration-delayed-notification` (on `DomainApprovalNotificationFailed`) follow the `send-provisioning-failed-notification` sibling exactly: `get_item` by `domainName`, read `ownerEmail` and `secondaryOwnerEmails` from the record. Neither event payload carries emails, and the domain record is guaranteed to exist in both cases (`ACTIVE` and `APPROVAL_NOTIFICATION_FAILED` respectively). No workflow ASL changes.

### D4: `RegisterDomainFailed` gains an additive `secondaryOwnerEmails` field

The registration-failed path is the one case where a DynamoDB record may not exist (validation failure, duplicate name), so the event detail is the only recipient source. `register.rs::emit_failed` already has the request in hand; the field is additive and non-breaking. `DomainRegistrationApproved` and `DomainRegistrationRejected` already carry `secondaryOwnerEmails`.

### D5: Single SES send with all recipients in `to_addresses`

Owner + secondary owners go in one `SendEmail` call. Trade-off accepted: a permanently rejected address (e.g. unverified in sandbox) drops the whole send for that event — logged at ERROR by the existing permanent-failure classification, invocation acknowledged. Independent per-recipient sends were considered and rejected as complexity ahead of need; the notification architecture revisit will reopen delivery semantics anyway.

Recipient assembly is duplicated across six Lambdas, so a small shared helper goes in `nexus-common` (extract owner + secondaries from an event detail `Value`, and from a DynamoDB item), keeping the six call sites one line each.

### D6: Owner and operator paths are fully independent

The operator path is rule → SNS; the owner path is rule → Lambda → SES. Separate rules, separate targets, separate DLQs. A failure on one path cannot suppress the other. In particular, for `DomainApprovalNotificationFailed` the operator learns via SNS even if the owner email fails for the same SES reason that caused the event.

### D7: Everything gated by the existing `NotificationsEnabled` condition

The SNS topic, subscription, transformer rule, and both new Lambdas deploy only when notifications are configured, matching every existing notification resource. No new parameters: `PlatformOperatorEmail` already exists.

## Risks / Trade-offs

- **[SNS email subscription requires a one-time confirmation click]** → Documented as a post-deploy step; until confirmed, operator messages are silently not delivered to that subscriber. The subscription resource itself deploys cleanly regardless.
- **[Operator email body is raw JSON, no subject line]** → Accepted while the email channel is simulated; the future channel-adapter change owns formatting.
- **[One bad recipient address drops the whole owner email]** → Existing permanent-failure handling logs at ERROR; accepted per D5.
- **[`DomainApprovalNotificationFailed` owner email may fail for the same systemic SES reason that fired the event]** → Operator path is SES-independent (D6); the Lambda failure lands in `LambdaFailureDlq` as backstop.
- **[Prefix-matched rule forwards future chatty events to operators]** → Accepted; volume is control-plane lifecycle only. The notification revisit can introduce filtering when channels arrive.

## Migration Plan

1. Deploy the core stack (new resources are additive, `NotificationsEnabled`-gated).
2. Operator clicks the SNS subscription confirmation email.
3. No data migration; no event contract breaks (one additive field on `RegisterDomainFailed`).

Rollback: remove the resources; no state to unwind (SNS subscription deletes with the topic).

## Open Questions

None — recipient strategy, operator scope (all lifecycle events), and change packaging were settled during exploration.
