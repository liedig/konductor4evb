## Context

The provisioning workflow (`provisioning.asl.yaml`) emits three EventBridge events: `DomainCreationStarted` (intermediate, no consumers), `DomainCreationSucceeded` (outcome), and `DomainCreationFailed` (outcome). The "Creation" naming is inaccurate — the domain record is created during registration; this workflow provisions infrastructure. The `isNewAccount` flag currently lives only on the started event.

## Goals / Non-Goals

**Goals:**

- Remove the intermediate `EmitCreationStarted` state and `DomainCreationStarted` event
- Rename outcome events to `DomainProvisioningSucceeded` / `DomainProvisioningFailed`
- Preserve the `isNewAccount` signal by moving it to the succeeded event
- Rename workflow states to match the new event naming

**Non-Goals:**

- Changing the provisioning workflow's resource creation logic
- Adding new events or states
- Modifying the saga compensation chain (it already references `EmitCreationFailed` which gets renamed)

## Decisions

### 1. Remove EmitCreationStarted entirely

The `DomainCreationStarted` event has no consumers. The `PROVISIONING` status on the domain record already signals that provisioning is underway. Removing this state eliminates one EventBridge API call per provisioning execution.

`CheckNewAccount` will transition directly to `CreatePlatformEventBus` instead of through `EmitCreationStarted`.

### 2. Move isNewAccount to the succeeded event

The `$isNewAccount` variable is assigned during `CheckNewAccount` and persists for the workflow duration. It will be included in the `DomainProvisioningSucceeded` event payload so downstream consumers (when they exist) can act on first-time account provisioning.

### 3. Rename states to match event names

| Current State | New State |
|---|---|
| `EmitCreationStarted` | (removed) |
| `EmitCreationSucceeded` | `EmitProvisioningSucceeded` |
| `EmitCreationFailed` | `EmitProvisioningFailed` |
| `UpdateDomainFailed` | `UpdateDomainProvisioningFailed` |

All `Next` references in the saga compensation chain pointing to `EmitCreationFailed` update to `EmitProvisioningFailed`.

### 4. No changes to compensation chain logic

The saga compensation chain is unchanged — only the terminal state name it routes to (`EmitCreationFailed` → `EmitProvisioningFailed`) is updated.

## Risks / Trade-offs

**[Breaking event contract]** → Any external system targeting `DomainCreationSucceeded` or `DomainCreationFailed` detail-types would break. Confirmed no consumers exist today, so risk is zero. The old event names were never published as a public contract.

**[Loss of started signal]** → Removing `DomainCreationStarted` means no explicit "provisioning began" event. This is acceptable because the `PROVISIONING` domain status serves the same purpose and is already set before the removed event was emitted.
