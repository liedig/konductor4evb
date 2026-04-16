## Why

The provisioning workflow emits a `DomainCreationStarted` intermediate event that has no consumers and adds an unnecessary state transition and EventBridge API call. The outcome events (`DomainCreationSucceeded`, `DomainCreationFailed`) use "Creation" naming which misrepresents what the workflow does — it provisions infrastructure, not creates the domain. The domain record already exists before the workflow runs. Renaming to `DomainProvisioningSucceeded/Failed` aligns with the existing `PROVISIONING` and `PROVISIONING_FAILED` domain status values.

## What Changes

- Remove the `EmitCreationStarted` state and `DomainCreationStarted` event from the provisioning workflow
- **BREAKING**: Rename `DomainCreationSucceeded` to `DomainProvisioningSucceeded`
- **BREAKING**: Rename `DomainCreationFailed` to `DomainProvisioningFailed`
- Rename workflow states: `EmitCreationSucceeded` → `EmitProvisioningSucceeded`, `EmitCreationFailed` → `EmitProvisioningFailed`, `UpdateDomainFailed` → `UpdateDomainProvisioningFailed`
- Move `isNewAccount` flag from the removed started event to the succeeded event payload

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `domain-provisioning-workflow`: Remove started event, rename outcome events from Creation to Provisioning, relocate isNewAccount flag
- `domain-management`: Update lifecycle events table to reflect renamed events and removal of started event

## Impact

- `domain-management/state-machines/provisioning.asl.yaml` — remove EmitCreationStarted state, rename outcome states and event detail-types
- `openspec/config.yaml` — update lifecycle event examples
- No Lambda code changes (no code references these event names)
- No API changes
- Breaking for any future consumers that might target `DomainCreationStarted/Succeeded/Failed` event detail-types — but no consumers exist today
