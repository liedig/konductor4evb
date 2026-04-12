## 1. Domain Model Changes

- [x] 1.1 Remove `Deleting`, `Deleted`, `DeleteFailed` variants from `DomainStatus` enum in `lib/control-plane-common/src/models.rs`
- [x] 1.2 Add `Deactivated` variant to `DomainStatus` enum with `DEACTIVATED` string representation
- [x] 1.3 Remove `DeletionMode` enum from `lib/control-plane-common/src/models.rs`
- [x] 1.4 Remove `ActiveDependencies` variant from `ControlPlaneError` in `lib/control-plane-common/src/error.rs` and its mapping in `api.rs` (if no longer used)

## 2. Update Domain Handler

- [x] 2.1 Add deactivation state transition logic to the update-domain handler: allow `desiredStatus: DEACTIVATED` from `ACTIVE` or `SUSPENDED`
- [x] 2.2 Reject transitions out of `DEACTIVATED` state with a conflict error
- [x] 2.3 Add EventBridge `RemovePermission` call when transitioning to `DEACTIVATED` to disable routing on the domain's platform event bus
- [x] 2.4 Emit `DomainDeactivated` event to the control plane event bus with payload: `domainId`, `domainName`, `ownerAccountId`, `deactivatedAt`, `deactivatedBy`, `previousStatus`

## 3. OpenAPI Spec Updates

- [x] 3.1 Remove the `DELETE /domains/{domainId}` operation from `api/openapi.yaml`
- [x] 3.2 Add `DEACTIVATED` to the `desiredStatus` enum in `UpdateDomainRequest` schema
- [x] 3.3 Remove the `deletionMode` query parameter and `DeleteDomainResponse` schema

## 4. SAM Template Cleanup

- [x] 4.1 Remove `DeleteDomainFunction` and `DeleteDomainLogGroup` resources from `template.yaml`
- [x] 4.2 Remove `CleanupWorkflow`, `CleanupWorkflowRole`, and the `DomainDeletionRequested` EventBridge rule from `template.yaml`
- [x] 4.3 Add `events:RemovePermission` permission to the UpdateDomain Lambda IAM policy, scoped to the domain bus name pattern
- [x] 4.4 Add `events:PutEvents` permission to the UpdateDomain Lambda IAM policy for the control plane event bus (for `DomainDeactivated` event)

## 5. Remove Delete Domain Artifacts

- [x] 5.1 Delete the `domain-management/functions/delete-domain/` directory
- [x] 5.2 Delete `domain-management/state-machines/cleanup.asl.json`
- [x] 5.3 Remove the `delete-domain` binary target from the Cargo workspace

## 6. Verify

- [x] 6.1 Run `cargo check` to confirm all code compiles with no errors
- [x] 6.2 Verify SAM template is valid with `sam validate`
