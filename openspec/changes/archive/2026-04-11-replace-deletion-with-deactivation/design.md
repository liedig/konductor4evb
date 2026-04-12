## Context

The control plane currently supports full domain deletion via `DELETE /domains/{domainId}`. This triggers a `CleanupWorkflow` Step Functions state machine that tears down all provisioned resources (event buses, schema registries, DLQs, dashboards, alarms) across platform and application accounts. The domain record transitions through `DELETING` -> `DELETED` (or `DELETE_FAILED`).

This design replaces deletion with deactivation: a terminal state that disables event routing while preserving all resources and audit history.

The `UpdateDomain` operation (via `desiredStatus: DEACTIVATED`) already handles state transitions between `ACTIVE` and `SUSPENDED`. Deactivation extends this mechanism with one additional terminal transition.

## Goals / Non-Goals

**Goals:**
- Remove the `DELETE /domains/{domainId}` endpoint and all associated infrastructure
- Add `DEACTIVATED` as a terminal domain status reachable from `ACTIVE` or `SUSPENDED`
- Disable event routing on deactivated domains by removing PutEvent permissions from domain event buses
- Preserve all provisioned resources for audit and forensic purposes
- Emit `DomainDeactivated` event for downstream consumers
- Remove the `CleanupWorkflow` state machine and its IAM role

**Non-Goals:**
- Implementing a scheduled resource cleanup for long-deactivated domains (future consideration)
- Providing a "reactivation" path from `DEACTIVATED` back to `ACTIVE` (terminal by design)
- Changing how `SUSPENDED` domains work (suspend remains reversible)
- Modifying the provisioning workflow

## Decisions

### Deactivation via UpdateDomain, not a dedicated endpoint

Deactivation is a state transition on the domain entity, not a separate operation. It uses the existing `PUT /domains/{domainId}` with `desiredStatus: DEACTIVATED`, consistent with how suspend/resume already works.

**Alternative considered**: A dedicated `POST /domains/{domainId}/deactivate` endpoint. Rejected because it would add API surface for what is fundamentally a status update, and the existing `desiredStatus` enum pattern already supports this cleanly.

### Deactivation disables routing by removing PutEvent permissions only

When a domain is deactivated, the control plane removes the PutEvent resource policy from the domain's platform-account event bus. This prevents any producer from publishing new events. The bus itself, schema registry, DLQs, dashboards, and all other resources remain intact.

**Alternative considered**: Deleting the event bus and recreating if ever needed. Rejected because bus deletion would lose any in-flight events and break the audit trail guarantee. Removing permissions is the least destructive way to stop routing.

**Alternative considered**: Adding a deny-all policy instead of removing the allow policy. Rejected because removing the allow policy achieves the same effect and is simpler to reason about -- no events can be published without an explicit allow.

### Deactivation is handled inline in the UpdateDomain handler

The permission removal (EventBridge `RemovePermission` API call) is performed by the UpdateDomain handler (or its SQS command handler successor) directly, not via a Step Functions workflow. The operation is a single API call and does not need the orchestration, retry, or multi-step coordination that provisioning requires.

**Alternative considered**: A dedicated deactivation Step Functions workflow. Rejected because the operation is a single `RemovePermission` call plus a DynamoDB status update -- far too simple to warrant workflow orchestration. If deactivation grows in complexity later (e.g., notifying subscribers, revoking cross-account roles), a workflow can be introduced at that point.

### DomainStatus enum changes

Remove: `Deleting`, `Deleted`, `DeleteFailed`
Add: `Deactivated`

The `DeletionMode` enum (used by the now-removed delete endpoint) is also removed from `models.rs`.

### CleanupWorkflow removal

The `CleanupWorkflow` state machine (`cleanup.asl.json`), its IAM role (`CleanupWorkflowRole`), and the `DomainDeletionRequested` EventBridge rule are all removed from `template.yaml`. The `delete-domain` Lambda function directory is deleted.

## Risks / Trade-offs

**[Risk] Resource costs accumulate for deactivated domains** -> Deactivated domains retain all provisioned resources (event buses, DLQs, dashboards). At scale with many deactivated domains, idle resources incur costs. Mitigation: most resources (EventBridge buses, SQS queues, CloudWatch dashboards) have negligible cost when idle. If this becomes material, a future change can introduce scheduled cleanup for domains deactivated beyond a retention threshold.

**[Risk] Deactivation is irreversible by design** -> If a team deactivates a domain by mistake, they must register a new one and go through the full approval + provisioning cycle. Mitigation: this is intentional -- deactivation is a governance action, not a casual toggle. The `SUSPENDED` status exists for temporary disablement. Clear API documentation and confirmation UX should reduce accidental deactivation.

**[Risk] UpdateDomain handler needs EventBridge permissions** -> The handler currently only writes to DynamoDB. Deactivation adds a dependency on EventBridge `RemovePermission`. The Lambda execution role needs the additional IAM permission. Mitigation: scope the permission to `events:RemovePermission` on the domain bus name pattern only.

## Migration Plan

1. Add `DEACTIVATED` to `DomainStatus` enum, remove `Deleting`/`Deleted`/`DeleteFailed`
2. Remove `DeletionMode` enum
3. Update `UpdateDomain` handler with deactivation logic and `DomainDeactivated` event emission
4. Add `events:RemovePermission` to the UpdateDomain Lambda IAM policy
5. Remove `DeleteDomain` Lambda function, log group, and IAM policies from SAM template
6. Remove `CleanupWorkflow`, `CleanupWorkflowRole`, and `DomainDeletionRequested` EventBridge rule from SAM template
7. Remove `delete-domain` function source directory and `cleanup.asl.json`
8. Update OpenAPI spec: remove DELETE operation, add `DEACTIVATED` to `desiredStatus` enum, remove `deletionMode` parameter
9. Update `from_control_plane_error` and error handling if `ActiveDependencies` error is no longer needed

**Rollback**: Since this removes an API endpoint, rollback requires redeploying the previous version of the SAM stack. No data migration is needed -- domain records in DynamoDB are unaffected.
