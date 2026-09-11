## Why

Customers deploying Konductor for Amazon EventBridge need a coherent way to supply their organisation identifier, domain environments, notification addresses, and platform-owner access configuration. Some settings already exist as deployment parameters, but customers need an explicit setup and configuration experience with validation and clear rules for changing settings after resources exist.

## What Changes

- Define a customer-facing configuration contract for the existing organisation identifier (`{org}`), domain environments (`{env}`), resource namespace pattern, notification sender address, and platform-operator recipient address. Distinguish the short organisation identifier from the AWS Organization ARN.
- Provide a supported setup path that validates these settings and prerequisites before applying them, with actionable feedback for invalid or incomplete configuration.
- Include platform-owner onboarding and controlled group-to-role configuration. Preserve the distinction between platform administration, platform audit access, and domain resource roles; configuration must not provide a path for ordinary users to grant themselves platform access.
- Classify settings by lifecycle: initial setup, safely updateable settings, and identity/topology-affecting settings that must be rejected or handled through an explicit migration process. Changing organisation identifiers, namespace patterns, or environment membership must not silently rename, replace, orphan, or delete existing resources.
- Preserve a single organisation-level Konductor control plane. The environment list describes domain environments, not separate platform instances.
- Keep event metadata composition separate. Platform metadata generators may consume resolved organisation, environment, and region values, but metadata revision activation, schema inheritance, and historical event validation belong to the separate metadata discussion. Configuration updates must not retroactively alter accepted event metadata.

This is an initial proposal-only capture, not an implementation-ready design. No configuration UI, file format, storage service, endpoint, or migration mechanism has been selected. No breaking change is approved by this proposal.

## Capabilities

### New Capabilities

None proposed. Extend existing architecture and control-plane access capabilities rather than introducing a new bounded context.

### Modified Capabilities

- `system-architecture`: Define customer setup and configuration behavior for organisation identity, domain environments, namespace naming, and notification settings, including validation and safe change boundaries.
- `control-plane-access`: Define customer platform-owner bootstrap and controlled group configuration while preserving existing global-role meanings and trusted identity boundaries.

## Impact

Observed implementation touchpoints:

- `konductor-core-stack.yaml` already declares `Organization`, `Environments`, `NamespacePattern`, `AwsOrganizationArn`, `NotificationSenderEmail`, and `PlatformOperatorEmail`. Environments default to `dev,staging,prod`.
- The core template creates fixed `platform-admin` and `platform-auditor` Cognito groups. `lib/konductor-common/src/principal.rs` also recognizes those literal names; group configurability cannot be treated as an isolated template rename.
- Deployment recipes in `Makefile`, notification provisioning, namespace resolution, and environment-specific API stages and domain resources will need to consume a consistent configuration contract.
- Existing architecture specs name `dev`, `staging`, and `prod` API stages. Customer-selected environment names must be reconciled with stage provisioning and request validation rather than assumed to work merely because `Environments` is parameterized.

No application code, deployed settings, group memberships, or main specs are changed by this capture. No new dependency is selected.

### Decisions Deferred

- Customer interaction model: deployment configuration, guided setup, administrative UI, or a combination.
- Source of truth and precedence between deployment inputs and any runtime settings.
- Exact updateability rules, including whether organisation identity is immutable after setup and how environment additions or removals are handled.
- Platform-owner bootstrap and recovery; whether customer groups map to fixed internal roles or customers manage membership in the existing controlled groups.
- Notification address validation, SES verification prerequisites, optional-notification behavior, and whether more than one operator recipient is needed.
- Configuration change authorization, audit history, activation, failure recovery, and behavior for existing installations.

Resolve these choices before creating delta specs, a design, or implementation tasks.
