# Proposal: rename-to-konductor

## Why

"Nexus" collides with an internal code name for an upcoming EventBridge feature, and "EventBridge Nexus" incorrectly suggests the project is an EventBridge product rather than a tool built on top of it. The project is private with a disposable dev deployment, so the rename is cheapest right now — before any external adoption makes the event-source and resource-name contracts costly to change.

The new name is **Konductor for Amazon EventBridge** (short form: **Konductor**).

## What Changes

- Rename the product everywhere: all prose, UI strings, email bodies, and documentation use "Konductor for Amazon EventBridge" (first mention / titles) and "Konductor" (short form). **BREAKING** for any reader of the old docs.
- Rename the event-source contract: lifecycle event sources `nexus.domain-management` and `nexus.event-streams` become `konductor.domain-management` and `konductor.event-streams`; the SNS forwarding rule prefix `nexus.` becomes `konductor.`. **BREAKING** for event consumers (none exist outside this repo).
- Rename all `nexus-`-prefixed AWS resources: stack `nexus-core` → `konductor-core`, table `nexus-domains` → `konductor-domains`, bus `{org}-nexus-bus` → `{org}-konductor-bus`, trust stack `nexus-trust` → `konductor-trust`, trust role `{org}-nexus-trust-role` → `{org}-konductor-trust-role`, SNS topic, Cognito pool, Lambda functions, queues, log groups, and the web bucket. **BREAKING**: the dev stack is torn down and redeployed; no data migration (nothing deployed is worth keeping).
- Rename code identifiers: crate `nexus-common` → `konductor-common`, `NexusConfig` → `KonductorConfig`, env vars `NEXUS_*` → `KONDUKTOR_*`, template file `nexus-core-stack.yaml` → `konductor-core-stack.yaml`, samconfig stack names.
- Rename the GitHub repository `evb-nexus` → `konductor4evb`, and the dev samconfig notification email aliases `sliedig+nexus@example.com` → `sliedig+konductor@example.com`.
- Update `AGENTS.md` product-naming section, the OpenSpec project glossary, and all spec prose that says "Nexus". Archived change folders under `openspec/changes/archive/` are dated history and are deliberately not rewritten.

## Capabilities

### New Capabilities

(none — the rename introduces no new behavior)

### Modified Capabilities

Only capabilities whose requirements name resources, event sources, or roles normatively get deltas; prose-only mentions are swept as tasks.

- `cross-account-trust-model`: trust role name `{org}-nexus-trust-role` → `{org}-konductor-trust-role`; trust stack and execution-role references.
- `platform-operator-notifications`: SNS topic `nexus-platform-notifications` → `konductor-platform-notifications`, display name, forwarding-rule source prefix `nexus.` → `konductor.`, lifecycle event sources.
- `domain-provisioning-workflow`: lifecycle event source `nexus.domain-management` → `konductor.domain-management`; platform resource pattern `{org}-nexus-*` → `{org}-konductor-*` (target bus `{org}-konductor-bus`).
- `installation`: stack names (`nexus-core` → `konductor-core`, `nexus-trust` → `konductor-trust`), stack-output names, tags `Application=nexus` → `Application=konductor`, and the smoke-test / make-target references.
- `resource-topology`: `{org}-nexus-bus`, `{org}-nexus-*` platform resource pattern, and the `nexus-event-streams` table name.
- `system-architecture`: normative resource-name references (event bus name `{org}-nexus-bus`, source prefix `nexus.`, authorizer naming).

The web console capability spec lives in the unarchived `add-nexus-web-ui` change, which SHALL be archived before this change is applied; its single brand mention ("Nexus-branded login page") is then swept as prose.

## Impact

- **Code**: every crate (`lib/nexus-common` rename touches ~75 files of imports), all Lambda functions, the web console, Makefile, CI workflows, scripts, and smoke tests.
- **APIs/events**: lifecycle event `source` values change — the only external-facing contract, with zero current consumers.
- **Deployment**: `nexus-core` (and any deployed trust stacks) are deleted; `konductor-core` is deployed fresh and dev personas are re-seeded.
- **Specs**: 6 deltas above plus a prose sweep across all remaining specs, docs, and `AGENTS.md`.
- **Out of scope**: archived change history, and any staged-rename or compatibility shims (nothing deployed needs to survive).
