# Proposal: rename-to-konductor

## Why

The former product name collided with an internal code name for an upcoming EventBridge feature, and it incorrectly suggested the project is an EventBridge product rather than a tool built on top of it. The project is private with a disposable dev deployment, so the rename was cheapest right now — before any external adoption makes the event-source and resource-name contracts costly to change. (The previous name is preserved in git history; this change record deliberately uses only the new naming.)

The new name is **Konductor for Amazon EventBridge** (short form: **Konductor**).

## What Changes

- Rename the product everywhere: all prose, UI strings, email bodies, and documentation use "Konductor for Amazon EventBridge" (first mention / titles) and "Konductor" (short form). **BREAKING** for any reader of the old docs.
- Rename the event-source contract: lifecycle event sources become `konductor.domain-management` and `konductor.event-streams`; the SNS forwarding rule source prefix becomes `konductor.`. **BREAKING** for event consumers (none exist outside this repo).
- Rename all formerly `konductor-`-prefixed AWS resources to the current names: stack `konductor-core`, table `konductor-domains`, bus `{org}-konductor-bus`, trust stack `konductor-trust`, trust role `{org}-konductor-trust-role`, SNS topic, Cognito pool, Lambda functions, queues, log groups, and the web bucket. **BREAKING**: the dev stack was torn down and redeployed; no data migration (nothing deployed was worth keeping).
- Rename code identifiers: crate `konductor-common`, `KonductorConfig`, env vars `KONDUKTOR_*`, template file `konductor-core-stack.yaml`, samconfig stack names.
- Rename the GitHub repository to `konductor4evb`, and the dev samconfig notification email aliases to `sliedig+konductor@example.com`.
- Update `AGENTS.md` product-naming section, the OpenSpec project glossary, and all spec prose. Archived change folders under `openspec/changes/archive/` were swept as well (user decision after implementation: absolute zero occurrences anywhere).

## Capabilities

### New Capabilities

(none — the rename introduces no new behavior)

### Modified Capabilities

Only capabilities whose requirements name resources, event sources, or roles normatively get deltas; prose-only mentions are swept as tasks.

- `cross-account-trust-model`: trust role name `{org}-konductor-trust-role`; trust stack and execution-role references.
- `platform-operator-notifications`: SNS topic `konductor-platform-notifications`, display name, forwarding-rule source prefix `konductor.`, lifecycle event sources.
- `domain-provisioning-workflow`: lifecycle event source `konductor.domain-management`; platform resource pattern `{org}-konductor-*` (target bus `{org}-konductor-bus`).
- `installation`: stack names (`konductor-core`, `konductor-trust`), stack-output names, tag `Application=konductor`, and the smoke-test / make-target references.
- `resource-topology`: `{org}-konductor-bus`, `{org}-konductor-*` platform resource pattern, and the `konductor-event-streams` table name.
- `system-architecture`: normative resource-name references (event bus name `{org}-konductor-bus`, source prefix `konductor.`, authorizer naming).

The web console capability spec lives in the unarchived `add-konductor-web-ui` change (per user decision it stays active — further web UI work is coming); its brand prose is swept in place.

## Impact

- **Code**: the crate rename touched ~75 files of imports, all Lambda functions, the web console, Makefile, CI workflows, scripts, and smoke tests.
- **APIs/events**: lifecycle event `source` values changed — the only external-facing contract, with zero consumers.
- **Deployment**: the old dev stacks were deleted; `konductor-core` was deployed fresh and dev personas re-seeded.
- **Specs**: 6 deltas above plus a prose sweep across all remaining specs, docs, and `AGENTS.md`.
- **Out of scope**: compatibility shims and staged renames (nothing deployed needed to survive).
