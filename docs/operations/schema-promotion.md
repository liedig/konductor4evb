# Schema promotion operations

[Documentation home](../README.md)

Operator reference for the promotion pipeline (`promote-schema-versions`).
Producer-facing usage lives in the [publishing guide](../guides/publishing-events.md#promoting-a-schema-to-later-environments);
the mechanics and rationale are in the [architecture page](../architecture/schemas.md#authoring-environment-and-promotion)
and the [decision record](../decisions/promote-schema-versions.md).

## What runs where

| Component | Name pattern | Role |
|-----------|--------------|------|
| API handler | `schema-promotions` Lambda | `POST`/`GET /schemas/promotions`; commits the head item |
| Stream consumer | `schema-promotion-stream` Lambda | Starts the workflow; emits `SchemaPromotion{Started,Completed,Failed}` |
| Replay worker | `schema-promotion-steps` Lambda | Plan, per-version replay, widen, narrow |
| Replay workflow | `PromoteSchemaWorkflow` state machine | VALIDATING → REPLAYING → REPINNING → terminal |
| Re-pin workflow | `RepinSchemaEnvWorkflow` state machine | Per-environment widen → drain → narrow |
| Rule writer | `materialize-subscription-env` Lambda (`repin` mode) | Rewrites rule patterns |

## Monitoring

- **`SchemaPromotionStreamDlq` alarm** (DLQ depth > 0): stream records that
  exhausted retries. Inspect the record's `NewImage`; the promotion id is the
  Step Functions execution name, so a failed start can be retried by putting
  the same record back (or by re-requesting the promotion after the head item
  is repaired).
- **Iterator age alarm** on the promotions table stream: the consumer is
  falling behind; promotions will appear stuck in `VALIDATING`.
- **Step Functions console**: `PromoteSchemaWorkflow` executions are named
  `promote-{promotionId}`; `RepinSchemaEnvWorkflow` executions are named
  `repin-{domain}-{eventName}-{env}-v{version}`. A failed execution's output
  names the failed state; the promotion head item carries the same failure.
- Promotion status is queryable any time:
  `GET /{stage}/schemas/promotions?domain=...&eventName=...` (consistent read
  of the head item).

## Recovery procedures

### Before promoting: drain the integration DLQs

Events that failed delivery sit in each domain's per-environment integration
DLQ. A promotion moves the subscription pins, so redrive (or discard) those
messages *before* promoting: after the pins move, an old-version event may no
longer match the narrowed rule when redriven. Check the DLQ depth alarms on
the consumer buses, redrive with the EventBridge console or
`aws events ...` redrive tooling, and confirm zero remaining messages.

### Promotion stuck in `VALIDATING`

The head item was committed but the workflow never started. Check the stream
DLQ and iterator age first. To retry, start the execution manually with the
idempotent name:

```bash
aws stepfunctions start-execution \
  --state-machine-arn arn:aws:states:${AWS_REGION}:${ACCOUNT}:stateMachine:PromoteSchemaWorkflow \
  --name "promote-${PROMOTION_ID}" \
  --input "{\"promotionId\":\"${PROMOTION_ID}\"}"
```

If the execution already exists, Step Functions answers
`ExecutionAlreadyExists`—safe to retry blindly.

### `FAILED` with reason `DIVERGED`

The target registry's version history is not a byte-identical prefix of the
source's (almost always a direct append in the target before this change, or a
registry rebuilt out of band). The target is untouched. Reconcile by
inspecting `ListSchemaVersions` + `DescribeSchema` in both registries, then
either delete and recreate the divergent target schema (destructive: re-pin
subscriptions afterwards) or accept the divergence permanently. The promotion
cannot proceed until the prefixes match.

### `FAILED` with reason `REPLAY_FAILED`

A single version failed to replay (registry throttling, content that no
longer compiles). The target keeps the versions already written. Fix the
cause, then re-request the promotion: replay resumes after the last written
version and skips everything before it.

### Re-pin failures after `COMPLETED`

`REPINNING` failures surface as per-entry `FAILED` outcomes in the
`RepinSchemaEnvWorkflow` Map output, as a `FAILED` status plus
`errorDetail` on the failing entry in `konductor-event-streams`, and in
the lifecycle event's `repin.failed` count; the promotion still completes.
Re-enabling the failed subscription entry repairs it (re-materialization
resolves the environment's latest version), and re-pinning is also retried
on the next append or promotion in that environment (the widen is
generation-guarded).

### Releasing a wedged promotion lease

The head item doubles as the per-target lease: while its phase is
non-terminal, new promotions for that schema and target are refused with
409. If a promotion is wedged in a non-terminal phase and the workflow is
gone or beyond repair (for example, the execution was deleted with the
state machine), cancel the lease manually, then stop the execution if one
is still running:

```bash
aws dynamodb update-item \
  --table-name konductor-schema-promotions \
  --key '{"pk":{"S":"billing#OrderNotification#staging"},"sk":{"S":"PROMOTION"}}' \
  --update-expression "SET phase = :cancelled, updatedAt = :now" \
  --condition-expression "promotionId = :pid" \
  --expression-attribute-values '{":cancelled":{"S":"CANCELLED"},":now":{"S":"'"$(date -u +%FT%TZ)"'"},":pid":{"S":"'"$PROMOTION_ID"'"}}'

aws stepfunctions stop-execution \
  --execution-arn "arn:aws:states:${AWS_REGION}:${ACCOUNT}:execution:PromoteSchemaWorkflow:promote-${PROMOTION_ID}"
```

The condition prevents clobbering a lease a newer request already took.
Only cancel a promotion whose execution is confirmed stopped or absent; a
cancelling-and-running pair can interleave replay writes.

### Tuning the drain window

`SchemaRepinDrainSeconds` (default 300, minimum 120) is the pause between
widening subscription rules to both versions and narrowing to the new one.
Raise it when consumers run long-polling or batched readers that may lag the
bus by minutes; lower it only when every consumer is synchronous.

## Durability notes

The promotions table retains head items for 90 days (TTL). The stream
consumer tolerates duplicates and restarts: workflow start is idempotent on
the execution name, and lifecycle events carry the committed state
transitions. The full classification is in the
[side-effects records](../development/side-effects/schemas.md#schema-promotion-stream).
