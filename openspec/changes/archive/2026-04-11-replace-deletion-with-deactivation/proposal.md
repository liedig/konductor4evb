## Why

Domains are long-lived organizational constructs that accumulate audit history, provisioned resources, and cross-account relationships. Permanent deletion breaks audit trails and makes forensic investigation of past event routing impossible. Replacing deletion with deactivation ensures domains are always recoverable and auditable while still allowing platform teams to disable routing for domains that are no longer active.

## What Changes

- **BREAKING**: Remove the `DELETE /domains/{domainId}` API operation entirely. Domains can no longer be deleted.
- **BREAKING**: Remove `DELETING`, `DELETED`, and `DELETE_FAILED` domain statuses from the lifecycle
- Add `DEACTIVATED` as a new terminal domain status, reachable from `ACTIVE` or `SUSPENDED` via the UpdateDomain operation (`desiredStatus: DEACTIVATED`)
- Deactivation disables event routing on the domain's event buses but preserves all provisioned resources (buses, schema registry, DLQs, dashboards) in place
- Remove the `CleanupWorkflow` Step Functions state machine, its IAM role, and the `DomainDeletionRequested` EventBridge rule
- Remove the `DeleteDomain` Lambda function, its log group, and associated IAM policies
- Remove `DomainDeletionRequested` and `DomainDeleted` events from the domain management event catalog
- Add `DomainDeactivated` event emitted when a domain transitions to `DEACTIVATED` status
- The `deletionMode` query parameter (previously `forceDelete`) is removed from the API

## Capabilities

### New Capabilities

_None_

### Modified Capabilities

- `domain-management`: Delete domain requirement replaced with deactivation. Domain lifecycle changes: `DELETING/DELETED/DELETE_FAILED` removed, `DEACTIVATED` added as terminal state. `DomainDeletionRequested` and `DomainDeleted` events replaced by `DomainDeactivated`. Update domain gains the ability to transition to `DEACTIVATED`.
- `domain-provisioning-workflow`: Cleanup workflow removed entirely. Deactivation does not tear down resources; it only disables routing.

## Impact

- **API contract**: `DELETE /domains/{domainId}` removed. Breaking change for any consumer using domain deletion.
- **OpenAPI spec** (`api/openapi.yaml`): Delete operation removed. `UpdateDomainRequest.desiredStatus` enum gains `DEACTIVATED`. `deletionMode` query parameter removed.
- **SAM template** (`template.yaml`): `DeleteDomainFunction`, `DeleteDomainLogGroup`, `CleanupWorkflow`, `CleanupWorkflowRole`, and associated EventBridge rule removed.
- **Lambda functions**: `delete-domain` function directory removed. `update-domain` (or its command handler successor) gains deactivation state transition logic.
- **Domain model** (`lib/control-plane-common/src/models.rs`): `DomainStatus` enum: remove `Deleting`, `Deleted`, `DeleteFailed`; add `Deactivated`. Remove `DeletionMode` enum.
- **State machines**: `cleanup.asl.json` removed.
- **EventBridge**: `DomainDeletionRequested` rule removed from control plane bus. New `DomainDeactivated` event type added.
