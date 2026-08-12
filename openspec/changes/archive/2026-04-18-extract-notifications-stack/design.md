## Context

The Konductor core stack (`template.yaml`) today requires two SES-related parameters, `ApprovalFromEmail` and `ApprovalToEmail`, with no defaults. The Lambdas that send approval and rejection emails assume these identities are already verified in SES. First-time installers must verify identities out of band before the core stack can deploy, and there is no path to install the core stack without SES at all (useful for local iteration or for operators who do not want email notifications yet).

SES is regional, and both identities must live in the same region as the notification Lambdas. Today this is implicit.

The existing capability `domain-owner-notifications` describes the behavior of approval and rejection notifications. This change moves *where* identities are managed without changing *how* notifications behave when enabled.

## Goals / Non-Goals

**Goals:**

- Make SES identity management an independently deployable concern (separate CloudFormation stack).
- Allow the Konductor core stack to deploy cleanly without SES configured, for testing and iteration.
- Preserve existing approval/rejection notification behavior when SES is configured.
- Align stack names and resource naming with the `{org}-{env}-{domain}` convention documented in `AGENTS.md`.

**Non-Goals:**

- Supporting multiple notification transports (Slack, SNS, webhooks). SES email remains the only transport.
- SES configuration sets, bounce handling, or production SES access. The notifications stack targets testing against the SES sandbox.
- Changing approval workflow semantics or task-token handling.
- API authz changes (deferred to a separate change).
- Changes to the Rust notification Lambda code.

## Decisions

### Decision 1: Sibling stack, not nested stack

Introduce `notifications-stack.yaml` as a standalone CloudFormation template deployed separately from core, rather than nesting it into `template.yaml` via `AWS::CloudFormation::Stack`.

**Rationale:** Separation of lifecycle. Operators can redeploy the core stack without touching SES identities (which have verification side effects), and can tear down notifications without affecting core. Nested stacks would entangle parameters and force SES to be understood at core-stack deploy time.

**Alternatives considered:**

- *Nested stack:* Simpler single-command deploy, but reintroduces the coupling we are trying to remove.
- *Conditional resources inside core:* Keeps one template but makes core harder to reason about and retains the verification side-effects in the core lifecycle.

### Decision 2: Gate core notification resources behind a CloudFormation condition

Add `NotificationsEnabled: !Not [!Equals [!Ref ApprovalFromEmail, ""]]` to core. The notification Lambdas, their log groups, the rejection EventBridge rule, and related permissions become conditional on `NotificationsEnabled`.

**Rationale:** Allows the core stack to deploy with no SES configured. The approval state machine still runs (task-token-based), but the notification dispatch path is absent. Since we currently have no alternate transport, this is explicitly a "testing/CI without email" mode, not a production mode.

**Trade-off:** Approval workflow with notifications disabled will wait on a task token with no email dispatched. Operators must approve via direct API call. This is acceptable for the intended use (local/testing).

**Alternatives considered:**

- *Make the parameters required as today:* Blocks first-time installs without SES.
- *Remove notifications entirely from core when disabled:* Rejected because it means swapping templates per environment.

### Decision 3: Notifications stack owns identity verification only

`notifications-stack.yaml` manages only `AWS::SES::EmailIdentity` resources for From and To addresses. No configuration set, no bounce/complaint topics, no Route53 records.

**Rationale:** Scope discipline. SES is used for test notifications in a sandbox account. Any additional SES features can be added later without changing the contract (the stack's outputs remain the raw email addresses).

**Outputs:**

- `FromEmail` (string, echo of the verified From address)
- `ToEmail` (string, echo of the verified To address)

Operators feed these outputs into the core stack as `ApprovalFromEmail` and `ApprovalToEmail` parameters.

### Decision 4: Same-region constraint is explicit

The `notifications-stack.yaml` description and the installation guide call out that the stack must deploy in the same region as the Konductor core stack. SES identities are regional, and the notification Lambdas call SES via the local region endpoint.

**Rationale:** Cross-region SES is out of scope. Making the constraint explicit prevents a subtle misconfiguration.

### Decision 5: Align stack name to `{org}-{env}-{domain}`

Update `samconfig.toml` default stack name to `acme-dev-konductor` (matching the `{org}-{env}-{domain}` pattern with `konductor` as the domain segment). The notifications stack name follows the same pattern: `acme-dev-konductor-notifications`. The trust stack pattern (already discussed): `acme-dev-konductor-trust`.

**Rationale:** Consistency with the naming convention documented in `AGENTS.md` and already used for AWS resources created by the stacks.

## Risks / Trade-offs

- **Risk:** An operator deploys the notifications stack in a different region than the core stack and receives no emails at runtime without an obvious error.
  - **Mitigation:** Document the same-region constraint in the template `Description`, installation guide prerequisites, and the notifications stack parameter help text.

- **Risk:** An operator deploys core without notifications and expects approval emails. The approval workflow silently waits for a manual decision call.
  - **Mitigation:** Document clearly in the installation guide that notifications are optional but recommended for end-to-end testing, and explain the manual `POST /domains/registrations/decide` path.

- **Risk:** Existing deployments of the `acme-dev-konductor`-style stack (or legacy `dev-evb-control-plane`) need a one-time rename. CloudFormation stacks cannot be renamed in place.
  - **Mitigation:** Call out in the installation guide's "Uninstall / Migrate" section that existing deployments must be torn down and re-deployed under the new name. No data loss risk since the `DomainsTable` has `DeletionPolicy: Retain`.

- **Risk:** SES sandbox enforces recipient verification. Installers testing end-to-end must verify both the From and To addresses, which is a manual SES console step.
  - **Mitigation:** The notifications stack creates the `AWS::SES::EmailIdentity` resources, which trigger SES verification emails. Installation guide walks through verification.

- **Trade-off:** When notifications are disabled, the installation is not fully end-to-end testable via email. Operators must approve via API. Acceptable for the intended testing-only mode.

## Migration Plan

1. Land this change (template + samconfig changes); ship without altering the existing spec behavior.
2. Existing deployments (if any) continue to work because `ApprovalFromEmail` / `ApprovalToEmail` remain valid parameters. For those deployments to drop SES, operators would need to pass empty strings on the next deploy.
3. New deployments follow the three-stack flow documented in the forthcoming `installation-guide` change.

Rollback: revert the template and samconfig changes; the notifications stack can be deleted independently without affecting core.

## Open Questions

None blocking. The following are deferred to later changes:

- Alternate transports (Slack, SNS, webhooks) behind a transport abstraction.
- Production SES concerns (configuration sets, bounce/complaint handling, DKIM).
- API authz posture and its impact on the smoke-test path.
