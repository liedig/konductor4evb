# SES identities extracted into optional sibling stack, conditional core deployment

[Documentation home](../README.md)

- Recorded date: 2026-04-17
- Change ID: `extract-notifications-stack`
- Area: Notifications
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

SES email identity management moved from the Konductor core stack into a standalone `notifications-stack.yaml` template. Core stack makes `NotificationSenderEmail`/`PlatformOperatorEmail` optional (empty-string defaults) and guards notification Lambda functions, log groups, EventBridge rules, and IAM permissions behind a `NotificationsEnabled` CloudFormation condition. When notifications are disabled, the core stack deploys without SES resources; the approval workflow still runs (task-token-based) but operators must approve via direct API call. Same-Region constraint: the notifications stack must deploy in the same Region as the core stack because SES identities are Regional. Design intent: decouple core deployment from SES verification lifecycle, enable testing without email, and leave the door open for alternate notification transports later.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-04-18-extract-notifications-stack/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
