## Why

SES identity verification is a deployment-time concern that shouldn't be entangled with the Nexus core stack. Today the core template takes `ApprovalFromEmail` and `ApprovalToEmail` as required parameters and assumes the operator has verified those identities out of band, which blocks first-time installs and couples core deployment to SES. Extracting identity management into an optional sibling stack makes SES a swappable transport for testing while leaving the door open for other notification transports later.

## What Changes

- Add a new `notifications-stack.yaml` CloudFormation template that manages SES email identities (From + To) for approval/rejection notifications. Minimal by design: no configuration set, no bounce/complaint handling.
- The notifications stack deploys in the same region as the Nexus core (SES is regional); this constraint is documented in the template description and the installation guide.
- Keep the approval/rejection notification Lambdas (`send-approval-notification`, `send-rejection-notification`) and their EventBridge rules in the core stack. They remain the default transport.
- Core `template.yaml` continues to accept `ApprovalFromEmail` / `ApprovalToEmail` parameters, but identity verification is no longer the core stack's responsibility. When these are blank, the notification Lambdas and rules are skipped via a CloudFormation condition so the core stack deploys cleanly without SES.
- Align `samconfig.toml` stack name to the `{org}-{env}-{domain}` convention: `acme-dev-nexus` (replacing the legacy `dev-evb-control-plane`).
- **BREAKING** for existing deployments that relied on the core stack requiring SES parameters: operators now deploy the notifications stack first (or skip it), then pass the resulting email addresses into the core stack.

## Capabilities

### New Capabilities

None. Notifications remain part of the existing `domain-owner-notifications` capability.

### Modified Capabilities

- `domain-owner-notifications`: SES identity ownership moves out of core into an optional sibling stack. Notifications become conditionally deployed in core so the stack can install without SES. Behavior when SES is configured is unchanged.

## Impact

- New file: `notifications-stack.yaml`.
- Modified: `template.yaml` (make `ApprovalFromEmail`/`ApprovalToEmail` optional; gate notification Lambdas and rules on a `NotificationsEnabled` condition).
- Modified: `samconfig.toml` (stack name alignment to `{org}-{env}-{domain}` pattern).
- No Rust code changes; notification Lambdas remain as-is.
- No API contract changes.
- Install flow becomes: notifications stack (optional) → core stack → per-account trust stack.
- Downstream: the `installation-guide` change depends on this work and will document the new three-stack install flow.
