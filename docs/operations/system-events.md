# System event recovery

[Documentation home](../README.md)

Failure recovery for the platform-to-domain notification pipeline. The event
model and contract versions are documented under
[System events](../reference/system-events.md); this page covers what an
operator does when delivery breaks.

## Pipeline recap

Committed upgrade snapshots (`upgradeEvents` on the event-streams table) are
projected by `konductor-system-event-projector` onto the
`{org}-konductor-system-bus` EventBridge bus, routed to per-domain SNS topics,
and captured in the `{org}-konductor-system-events-archive` archive (90 days,
audit only). Contracts live in the `{org}-konductor-system-schemas` registry,
registered at deploy time by the `konductor-system-schema-bootstrap` custom
resource.

## Alarms and operator actions

| Alarm | Meaning | Action |
|---|---|---|
| `konductor-system-event-projector-iterator-age-alarm` | The projector is falling behind the event-streams table stream. | Check the projector's logs and the errors alarm. Records remain in the stream for 24 hours; recover before that window closes or the events are lost to the stream (see S3 recovery below for anything already failed out). |
| `konductor-system-event-projector-errors-alarm` | Projector invocations are failing; retries and batch bisect are in progress. | Read the function logs for the failing record. After bounded retries and bisect, the offending batch is delivered to the S3 failure bucket; follow the targeted recovery below. |

## Targeted failed-message recovery (S3 failure bucket)

When a batch exhausts its retries, Lambda delivers the full invocation payload,
including the original stream records, to
`s3://konductor-system-event-failures-{account}-{region}/`. Objects are
SSE-S3 encrypted, public access is blocked, and lifecycle expiry deletes them
after 90 days, well beyond the 24-hour stream retention window. The bucket name
carries no `{org}` prefix: the configuration contract allows 43-character
organizations, and an org-prefixed name with account and region appended would
exceed S3's 63-character limit.

1. Fix the root cause first (for example, restore the system bus or correct a
   malformed `upgradeEvents` snapshot). Redriving before the fix repeats the
   failure.
2. List failed objects and extract the original records:

   ```bash
   aws s3 ls "s3://konductor-system-event-failures-{account}-{region}/" --recursive
   aws s3 cp "s3://konductor-system-event-failures-{account}-{region}/<key>" - \
     | jq '{ Records: .DDBStreamBatchInfo.records }' > redrive-payload.json
   ```

3. Re-invoke the projector directly with the extracted payload:

   ```bash
   aws lambda invoke --function-name konductor-system-event-projector \
     --payload fileb://redrive-payload.json redrive-result.json
   ```

4. Confirm `redrive-result.json` reports no `batchItemFailures`, then delete
   the recovered object from the bucket.

Redrive preserves identity. The projected events carry the same deterministic
`idempotencyKey` (`system-event/v1/…`) as the original attempt. A redrive that
partially succeeded before failing is therefore safe, and consumers that
deduplicate on the idempotency key see no semantic duplicate.

## What the archive is not

The 90-day EventBridge archive is an audit trail. There is **no archive replay
procedure** and no automatic historical catch-up in this release. Do not
reconstruct missed deliveries from the archive. Use the S3 failure bucket for
targeted recovery. Events lost beyond both the stream window and the
failure-bucket retention are a notification gap: reconcile it from the source
of truth (the event-streams table's current pins and `upgradeEvents`
snapshots).

## Distinct failure paths

- **Stream processing failures** (projector cannot publish): bounded retries,
  bisect, then the S3 failure bucket above. This is the path the projector
  alarms cover.
- **EventBridge target-invocation failures** (the bus cannot deliver to the
  router or a topic rule target): each target has its own SQS dead-letter
  queue with the platform-standard 14-day retention (the SQS maximum) and a
  depth alarm. These queues are separate from the projector's Lambda failure
  destination on purpose: one path protects projection, the other protects
  fan-out.
- **Consumer endpoint delivery failures** (SNS cannot deliver to a subscriber's
  SQS queue or Lambda): the endpoint owner's responsibility; their
  subscription's own DLQ and redrive policy apply.

## Deploy-time contract registration failures

The `SystemSchemaBootstrap` custom resource fails the deployment when the
checked-in manifest (`api/system-events/manifest.json`) does not match the
registry (drift, unexpected version history, or an incompatible definition).
Do not edit the registry out of band to unblock the deploy; reconcile the
checked-in schemas and manifest, then redeploy. A no-op deploy where the
registry already matches succeeds silently.

## Rollout and rollback

System events roll out dark and are enabled deliberately:

1. **Deploy the core stack.** The system bus, archive, registry, projector,
   and bootstrap custom resource deploy unconditionally and are independent of
   the optional SES notification condition. The
   `SystemEventEmissionEnabled` parameter defaults to `false`: no
   `upgradeEvents` snapshots are written, so the projector has nothing to
   publish.
2. **Verify contract registration.** Check the
   `konductor-system-schema-bootstrap` log group: it verifies the checked-in
   manifest and registers `SubscriptionUpgradeStarted` v1 and
   `SubscriptionUpgraded` v1 in `{org}-konductor-system-schemas`. The
   deployment fails if the registry drifts from the checked-in contracts.
3. **Backfill existing domains.** Provisioned-before-rollout domains have no
   topic. Run the idempotent backfill (dry-run first):

   ```bash
   AWS_REGION=<region> DRY_RUN=1 ./scripts/backfill-system-event-topics.sh
   AWS_REGION=<region> ./scripts/backfill-system-event-topics.sh
   ```

   Select specific domains with `DOMAINS="name1 name2"`. Per-domain failures
   are reported and retryable; the script preserves lifecycle status, existing
   endpoint subscriptions, and all other resources.
4. **Enable emission.** Set `system_event_emission_enabled = true` in
   `konductor-platform.toml` (or pass the
   `SystemEventEmissionEnabled=true` stack parameter) and update the stack.
   New upgrade phases start emitting; nothing is backfilled historically.
5. **Watch the alarms** in the table above during the first upgrades.

Quotas to check before rollout: SNS topics per account (one per domain), and
EventBridge buses/archives per account (one each). See
[service quotas](service-quotas.md).

**Rollback is non-destructive.** Setting `SystemEventEmissionEnabled` back to
`false` stops new snapshots and therefore new events; existing topics,
subscriptions, the registry, and the archive are retained. Removing the
feature's stack resources entirely is only safe after disabling emission and
retiring every domain topic (via `scripts/teardown-domain.sh` per domain or
the backfill-era tooling), and is not part of the standard rollback.

### Cost-center retagging

The `retag-domain` workflow retags the recorded topic with the domain's
current cost center. Domains without a recorded ARN (not yet backfilled or
newly registered with the gate off) are skipped as a no-op. Disabling a
domain retains its topic and any existing endpoint subscriptions.
