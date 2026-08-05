# Tasks: add-lifecycle-outcome-notifications

## 1. Shared recipient helper

- [x] 1.1 Add owner-recipient helpers to `nexus-common` (extract `ownerEmail` + `secondaryOwnerEmails` from an event detail `Value` and from a DynamoDB item; empty/missing secondaries degrade to owner-only), with unit tests

## 2. Operator SNS channel

- [x] 2.1 Add `nexus-platform-notifications` SNS topic (DisplayName `Nexus Platform`), topic policy for `events.amazonaws.com` scoped to the forwarding rule, and email subscription for `PlatformOperatorEmail` to `nexus-core-stack.yaml`, gated by `NotificationsEnabled`
- [x] 2.2 Add the lifecycle-forwarding EventBridge rule on the nexus bus (`source` prefix `nexus.`) with input transformer (JSON message: detail-type, source, time, detail) and `DeadLetterConfig` on `EventBridgeRuleFailureDlq`

## 3. Provisioning-succeeded owner notification

- [x] 3.1 Create `send-provisioning-succeeded-notification` crate (DDB lookup by `domainName`, owner + secondary recipients, HTML "domain is live" email, permanent-failure classification), following the provisioning-failed sibling; add to workspace members; include body/recipient unit tests
- [x] 3.2 Wire function, log group, EventBridge rule on `DomainProvisioningSucceeded`, Lambda permission, and DLQ config into `nexus-core-stack.yaml` under `NotificationsEnabled`

## 4. Registration-delayed owner notification

- [x] 4.1 Create `send-registration-delayed-notification` crate (DDB lookup by `domainName`, owner + secondary recipients, HTML "registration delayed, platform team notified" email with no technical detail, permanent-failure classification); add to workspace members; include body/recipient unit tests
- [x] 4.2 Wire function, log group, EventBridge rule on `DomainApprovalNotificationFailed`, Lambda permission, and DLQ config into `nexus-core-stack.yaml` under `NotificationsEnabled`

## 5. Secondary owners in existing notifications

- [x] 5.1 Add `secondaryOwnerEmails` to the `RegisterDomainFailed` event detail in `register.rs::emit_failed`, with test coverage
- [x] 5.2 Update `send-approval-outcome-notification` and `send-rejection-outcome-notification` to address owner + secondaries from the event detail via the shared helper
- [x] 5.3 Update `send-registration-failed-notification` to address owner + secondaries from the event detail via the shared helper
- [x] 5.4 Update `send-provisioning-failed-notification` to address owner + secondaries from the DynamoDB record via the shared helper

## 6. Documentation and verification

- [x] 6.1 Update the event catalog Triggers column in `docs/development/architecture.md` for `DomainProvisioningSucceeded`, `DomainApprovalNotificationFailed`, and the operator SNS forwarding; note the one-time SNS subscription confirmation step in deployment docs
- [x] 6.2 Run `make check` (fmt, clippy) and `cargo test` across the workspace; validate the SAM template builds (`sam validate` or `make` target if present)
