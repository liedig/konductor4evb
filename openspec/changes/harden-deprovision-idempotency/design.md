# Design: harden-deprovision-idempotency

## Context

`deprovision-env.asl.yaml` deletes one environment's resources in sequence (integration DLQ, app bus, alarms, dashboard, subscription DLQ, schema registry, platform bus). Every step has `Catch: [States.ALL] → append to compensationErrors → continue`, and the workflow always ends `SUCCEEDED`. The spec's "Deprovisioning is idempotent" scenario requires not-found responses to be successful no-ops, but the blanket catch cannot distinguish them from real failures. `scripts/teardown-domain.sh` now treats non-empty `compensationErrors` as "resources left behind" and refuses to delete the domain record, so every false positive blocks cleanup.

Observed false positives: schema registry 404 recorded as failure; `DeleteIntegrationDlq`/`DeleteSubscriptionDlq` failing on undefined JSONata paths when `resourceArns` lacks those keys.

## Goals / Non-Goals

**Goals:**

- `compensationErrors` non-empty if and only if a resource that should have been deleted still exists.
- Absent resource identifiers in the input are skipped as no-ops.
- No behavior change for genuine failures (still collected, workflow still never hard-fails).

**Non-Goals:**

- Changing the workflow's collect-and-continue posture or its output contract.
- Extending deprovisioning scope (subscription rules on other buses, event-streams records); that belongs to a future domain-deletion capability.
- Teardown script changes (it already consumes `compensationErrors` correctly).

## Decisions

**D1: Per-step not-found catches ahead of the States.ALL catch.**
Each step gets `Catch: [<service not-found errors>] → continue (no error recorded)` before the existing `Catch: [States.ALL]`. Not-found error names per service: `Schemas.NotFoundException`; SQS `DeleteQueue` on a missing queue (`Sqs.QueueDoesNotExistException` / non-existent queue error); EventBridge `Events.ResourceNotFoundException`; CloudWatch `DeleteAlarms` is already a silent no-op for missing alarms and `DeleteDashboards` surfaces `CloudWatch.ResourceNotFoundException` / `CloudWatch.DashboardNotFoundError`. Exact SDK-integration error names to be confirmed against live behavior during verification (task-gated), since aws-sdk service integrations surface service exception names, not SDK enum names.

Alternative rejected: JSONata error-message matching inside the States.ALL catch (`$contains($states.errorOutput.Cause, 'does not exist')`), brittle string matching across services and message formats.

**D2: Guard steps on input presence with a Choice state (or JSONata existence check) per optional identifier.**
`DeleteIntegrationDlq` and `DeleteSubscriptionDlq` take queue URLs from `resourceArns`; when the key is absent the JSONata expression returns undefined and the state fails before the SDK call. A presence check routes straight to the next step. Name-derived steps (buses, registry, alarms, dashboard) don't need guards, their identifiers are computed from `domainName`/`environment`.

Alternative rejected: defaulting missing URLs to empty strings and letting the SDK error, indistinguishable from real failures and noisy.

## Risks / Trade-offs

- [Wrong error name in a not-found catch silently reclassifies a real failure as no-op] → verification tasks exercise both directions: not-found produces empty `compensationErrors`, and a genuinely undeletable resource still gets recorded.
- [Catch-order subtleties in ASL] → specific-before-States.ALL ordering is the documented Step Functions catch semantics; cfn-lint plus a dev execution validate.

## Migration Plan

1. Single core-stack deploy; the workflow is versionless and the change is backward compatible (same input/output contract).
2. Rollback: redeploy previous template.

## Open Questions

- Exact error names surfaced by the aws-sdk Step Functions integrations for SQS missing-queue and CloudWatch missing-dashboard (confirm during 3.x verification; the catch lists may need the service's literal exception strings).
