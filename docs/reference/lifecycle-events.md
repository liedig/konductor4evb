# Lifecycle events

[Documentation home](../README.md)

Internal control-plane events, not the business events delivered to subscribers.

## Event bus and lifecycle events

The **konductor bus** (`konductor-bus`) carries all internal lifecycle events. It is a platform resource—a single bus per Konductor deployment, not per env. It is the backbone for workflow orchestration—events trigger Step Functions executions and EventBridge rules invoke notification Lambdas.

### Event catalog

| Detail type | Source | Emitted by | Triggers |
|------------|--------|-----------|----------|
| `RegisterDomainSucceeded` | `konductor.domain-management` | Command handler (register) | Not applicable |
| `RegisterDomainFailed` | `konductor.domain-management` | Command handler (register) | Registration-failed notification Lambda |
| `UpdateDomainSucceeded` | `konductor.domain-management` | Command handler (update) | Not applicable |
| `UpdateDomainFailed` | `konductor.domain-management` | Command handler (update) | Not applicable |
| `DomainRegistrationApproved` | `konductor.domain-management` | Approval workflow | Provision-domain workflow (outer); approval-outcome notification Lambda |
| `DomainRegistrationRejected` | `konductor.domain-management` | Approval workflow | Rejection notification Lambda |
| `DomainApprovalNotificationFailed` | `konductor.domain-management` | Approval workflow | Registration-delayed notification Lambda (owner-facing, no technical detail) |
| `DomainProvisioningSucceeded` | `konductor.domain-management` | Provision-domain workflow | Provisioning-succeeded notification Lambda |
| `DomainProvisioningFailed` | `konductor.domain-management` | Provision-domain workflow | Provisioning-failed notification Lambda |
| `SubscribeSucceeded` / `SubscribeFailed` | `konductor.event-streams` | Subscription command handler | Not applicable |
| `UpdateSubscriptionSucceeded` / `UpdateSubscriptionFailed` | `konductor.event-streams` | Subscription command handler | Not applicable |
| `UnsubscribeSucceeded` / `UnsubscribeFailed` | `konductor.event-streams` | Subscription command handler | Not applicable |
| `SubscriptionActivated` | `konductor.event-streams` | Activate-subscription workflow | Not applicable |
| `SubscriptionActivationFailed` | `konductor.event-streams` | Activate-subscription workflow | Not applicable |
| `SubscriptionRemoved` | `konductor.event-streams` | Remove-subscription workflow | Not applicable |
| `SchemaVersionPublished` | `konductor.schema-management` | Schema-admin (after a committed create/append) | Subscription-extender Lambda |
| `SubscriptionEnvironmentExtended` | `konductor.event-streams` | Subscription-extender Lambda | Not applicable |

The `DomainProvisioningFailed` payload carries `failedEnvironment` (or `null` for domain-level failures), `failedStep`, `errorCode`, `errorMessage`, and an aggregated `compensationErrors` array from any rollback runs. `SubscriptionActivationFailed` carries `failedEnvironment`, `failedStep`, and `errorMessage` (no compensation errors—activation has no rollback; per-environment outcomes ride on both subscription lifecycle events).

Domain-management events use the source `konductor.domain-management`; subscription events use `konductor.event-streams`. The `DomainRegistrationApproved` event is the key orchestration trigger—it initiates the provisioning workflow via an EventBridge rule on the konductor bus.

**Platform operator notifications.** In addition to the per-event owner notifications above, every lifecycle event on the konductor bus (any `source` with the `konductor.` prefix) is forwarded to the `konductor-platform-notifications` SNS topic by a single EventBridge rule with an input transformer, giving platform operators full lifecycle visibility through one subscription point. `PlatformOperatorEmail` is subscribed at deploy time (one-time SNS confirmation required); future notification channel adapters (Slack, SMTP relays) attach as additional topic subscribers. The operator path is SNS-only and independent of SES—in particular, `DomainApprovalNotificationFailed` still reaches operators when SES delivery is the failed dependency.
