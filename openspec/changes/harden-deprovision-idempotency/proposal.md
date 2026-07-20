# Proposal: harden-deprovision-idempotency

## Why

The main spec already requires deprovision-env to treat "resource not found" as a successful no-op, but the implementation catches `States.ALL` per step and records not-found errors in `compensationErrors` alongside genuine failures. Since teardown tooling now inspects `compensationErrors` to decide whether resources were left behind, false positives block record deletion and force manual investigation (observed live: a schema-registry 404 reported as a teardown failure). Steps whose resource ARN is absent from the input also fail on undefined JSONata paths instead of no-oping.

## What Changes

- Bring `deprovision-env.asl.yaml` into conformance with the existing idempotency requirement: each delete step catches its service-specific not-found error (`Schemas.NotFoundException`, SQS queue-does-not-exist, `Events.ResourceNotFoundException`, CloudWatch no-op semantics) as a silent no-op before the `States.ALL` catch records real failures.
- Steps whose resource identifier is absent from the input `resourceArns` set (never provisioned, or lost mid-saga) skip as no-ops instead of failing on undefined JSONata expressions.
- `compensationErrors` becomes a true signal: non-empty means resources were actually left behind.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `domain-provisioning-workflow`: the per-environment deprovisioning requirement gains an explicit partial-input scenario (absent resource identifiers are skipped as no-ops); the existing not-found-is-a-no-op scenario is unchanged and the implementation is brought into conformance with it.

## Impact

- `domain-management/state-machines/deprovision-env.asl.yaml` only (per-step `Catch` chains and input guards).
- No Rust code, API, or schema changes; teardown tooling (`scripts/teardown-domain.sh`) already consumes `compensationErrors` and benefits without modification.
- Behavior of the provisioning saga's rollback path improves identically (it invokes the same workflow).
