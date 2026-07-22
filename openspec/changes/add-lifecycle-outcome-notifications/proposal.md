# Proposal: add-lifecycle-outcome-notifications

## Why

Two lifecycle outcomes currently vanish into the void: `DomainProvisioningSucceeded` has no consumers (owners are told their registration was approved but never that their domain is live), and `DomainApprovalNotificationFailed` is described as operator-facing but nothing watches it (a domain stuck in `APPROVAL_NOTIFICATION_FAILED` is invisible until someone goes looking). Additionally, every owner-facing notification emails only `ownerEmail`, silently ignoring the `secondaryOwnerEmails` collected at registration. Platform operators also have no visibility into lifecycle outcomes at all — they see approval requests but never learn what happened afterwards.

## What Changes

- **New platform operator notification channel via SNS.** A `nexus-platform-notifications` SNS topic with an email subscription for `PlatformOperatorEmail`, fed by a single EventBridge rule on the nexus bus matching all lifecycle events (`source` prefix `nexus.`), with an input transformer producing readable plain-text messages. Zero new Lambdas for the operator path. SNS is the deliberate extensibility seam: future channel adapters (Slack, Outlook/SMTP, webhooks) become additional topic subscribers in separate changes.
- **New owner notification: domain is live.** A `send-provisioning-succeeded-notification` Lambda triggered by an EventBridge rule on `DomainProvisioningSucceeded`, emailing the owner that their domain is provisioned and active (following the existing one-Lambda-per-event SES pattern with recipient resolution via DynamoDB lookup).
- **New owner notification: registration delayed.** A Lambda triggered by `DomainApprovalNotificationFailed`, emailing the owner that their registration is delayed and the platform team has been notified. Owner and operator messages flow independently — a failure on one path does not suppress the other.
- **Secondary owners included in all owner-facing notifications.** Every owner-facing notification Lambda (approval outcome, rejection outcome, provisioning failed, registration failed, plus the two new ones) sends to `ownerEmail` and `secondaryOwnerEmails` in a single SES send.
- **Documentation.** Update the event catalog in `docs/development/architecture.md` (Triggers column) for the newly consumed events, and note the one-time SNS subscription confirmation step in deployment docs.

Explicitly deferred (future changes): notification channel adapter implementations (Slack, Outlook), per-domain owner channel preferences, and any broader notification-architecture rework.

## Capabilities

### New Capabilities

- `platform-operator-notifications`: Operator-facing notification channel — the SNS topic, its operator email subscription, the all-lifecycle-events EventBridge rule with input transformer, and the conditional deployment behavior. Distinct from owner notifications: different audience, different channel (SNS vs SES), source-agnostic (covers `nexus.domain-management` and `nexus.event-streams` events alike).

### Modified Capabilities

- `domain-owner-notifications`: Three requirement changes — (1) new provisioning-succeeded notification to owners, (2) new approval-notification-failed notification to owners, (3) all owner-facing notifications include `secondaryOwnerEmails` alongside `ownerEmail` in a single SES send.

## Impact

- **Infrastructure (`nexus-core-stack.yaml`)**: new SNS topic, topic policy (EventBridge publish), email subscription, one EventBridge rule with input transformer; two new Lambda functions with log groups, rules, permissions, and DLQ wiring — all gated by the existing `NotificationsEnabled` condition.
- **Code**: two new Rust Lambda crates under `domain-management/functions/`; recipient-list changes in four existing notification Lambdas (approval outcome, rejection outcome, provisioning failed, registration failed).
- **Operations**: SNS email subscriptions require a one-time confirmation click by the operator after deployment (unlike SES identities). Operator emails are plain text; owner emails remain HTML via SES.
- **No event payload changes**: both new consumers resolve recipients via `domainName` DynamoDB lookup, so no workflow ASL or emitted event contracts change. No breaking changes.
