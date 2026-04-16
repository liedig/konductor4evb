## 1. Remove EmitCreationStarted state

- [x] 1.1 Remove the `EmitCreationStarted` state from `provisioning.asl.yaml`
- [x] 1.2 Update `CheckNewAccount` to transition directly to `CreatePlatformEventBus` instead of `EmitCreationStarted`

## 2. Rename outcome event states and detail-types

- [x] 2.1 Rename `EmitCreationSucceeded` state to `EmitProvisioningSucceeded` and change DetailType from `DomainCreationSucceeded` to `DomainProvisioningSucceeded` in `provisioning.asl.yaml`
- [x] 2.2 Add `isNewAccount` flag (`{% $isNewAccount %}`) to the `EmitProvisioningSucceeded` event Detail payload
- [x] 2.3 Rename `EmitCreationFailed` state to `EmitProvisioningFailed` and change DetailType from `DomainCreationFailed` to `DomainProvisioningFailed` in `provisioning.asl.yaml`
- [x] 2.4 Rename `UpdateDomainFailed` state to `UpdateDomainProvisioningFailed` in `provisioning.asl.yaml`
- [x] 2.5 Update `UpdateDomainRecord` Next reference from `EmitCreationSucceeded` to `EmitProvisioningSucceeded`
- [x] 2.6 Update `CompensatePlatformEventBus` and `DeletePlatformEventBus` Default/Next references from `EmitCreationFailed` to `EmitProvisioningFailed`
- [x] 2.7 Update `EmitProvisioningFailed` Next reference from `UpdateDomainFailed` to `UpdateDomainProvisioningFailed`

## 3. Update config and documentation

- [x] 3.1 Update `openspec/config.yaml` context to replace `DomainCreationStarted` with `DomainProvisioningSucceeded` in the lifecycle events example list
- [x] 3.2 Update decision log at `docs/development/decisions.md` with: provisioning event naming rationale and removal of intermediate event
