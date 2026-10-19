# Design

## Context

See proposal.md (Why) for the motivation. The constraints below shape the approach.

- **Registries live in the platform account.** `schema-admin` reaches every environment's registry (`{org}-{env}-{domain}-schema-registry`) with one Schemas client and no role assumption. The environment comes from the API Gateway stage name. A promotion workflow can therefore read the source registry and write the target registry directly.
- **Version numbers are assigned by AWS Schemas.** `UpdateSchema` mints the next number, and no API sets it. Aligned numbering therefore depends on writing versions in order, exactly once each, with nothing else writing to the target registry at the same time.
- **Registries cannot replay history (spike 1.1).** `DescribeSchema`, `ListSchemaVersions`, and `ExportSchema` expose only the latest version's content or a conversion of it; there is no API to read version N once N+1 exists. Replay therefore requires a Konductor-side content record per version.
- **Registry content is normalized (spike 1.1).** `DescribeSchema` returns content with whitespace re-serialized, so equality must be judged on parsed JSON, never bytes. `UpdateSchema` with content equal to the latest version fails with `ConflictException` ("no change") and mints nothing; a retry with the same `ClientTokenId` returns the originally minted version.
- **Rules pin exactly one version today.** `materialize-subscription-env` resolves the latest version with `DescribeSchema` and writes `schemaVersion: [pinned]`. Publish admission stamps each event with the environment's latest version, so any new version stops matching existing rules.
- **The publish path retries for about 90 seconds.** `konductor-publish-queue` uses a 30-second visibility timeout with `maxReceiveCount: 3` before messages move to `konductor-publish-dlq`. An event stamped vN can reach the platform bus up to about 90 seconds after admission, or much later through a DLQ redrive.
- **Stages and CloudFront routes are hardcoded.** The SAM APIs create `dev`, and `staging`/`prod` are explicit `AWS::ApiGateway::Stage` resources on both APIs. CloudFront has fixed `/dev/*`, `/staging/*`, and `/prod/*` behaviors, and `config.json` hardcodes `apiBaseUrl: …/dev`. The configurable `Environments` parameter already accepts other names, but the API has no matching stages for them.
- **`subscription-extender` already consumes `SchemaVersionPublished`** through a control-plane bus rule with a DLQ. It skips environments that are already materialized.
- **Side-effects rubric** (AGENTS.md): a handler performs one synchronous commit, and other side effects run from the table's stream. Step Functions execution names are deterministic idempotency keys.

## Goals / Non-Goals

**Goals:**

- One writer per target registry at any time, so replay can rely on read-before-write instead of registry-level idempotency.
- Promotion state that is committed before any side effect runs, with workflow start and lifecycle events driven from the record's stream.
- One re-pin mechanism (widen, drain, narrow) shared by promotions and authoring appends, which stays correct when re-pins overlap.
- Environment-agnostic stage and route provisioning, so the authoring environment and promotion order follow the configured list.

**Non-Goals:**

- Consumer-facing notification of version changes (emails, system events). The `domain-system-events` change consumes the lifecycle events defined here.
- A promotion history beyond the most recent promotion per schema and target. The lifecycle events carry the audit trail.
- Rollback or demotion of versions. Registries stay append-only.
- Bulk promotion of several schemas in one request.
- Adding or removing environments on a live installation. The list stays fixed at setup.

## Decisions

### 1. The authoring guard returns 400, before any registry access

`schema-admin` compares the request stage with `environments[0]` (from the existing `ENVIRONMENTS` global function variable) before authorization side calls or registry reads. A mismatch returns `400 Bad Request` with code `AuthoringEnvironmentOnly`, naming the authoring environment.

- *Why 400:* the request is well formed but targets the wrong place, and no binding change can make it succeed. `403` would suggest that a grant is missing. `405` would misdescribe the route, which exists on every stage because all stages share one deployment.
- *Alternative:* removing the routes from later stages. This is not possible, because all stages share one deployment and one OpenAPI definition.

### 2. Two new tables: `konductor-schema-promotions` and `konductor-schema-versions`

The new table has a stream (`NEW_AND_OLD_IMAGES`), point-in-time recovery, and KMS encryption, matching the existing tables. Its key is `pk = {domain}#{eventName}#{environment}` and `sk ∈ {PROMOTION, REPIN}`:

- `PROMOTION` is the head item for the most recent promotion into that environment: `promotionId`, `source`, `target`, `fromVersion`, `requestedVersion`, `lastWrittenVersion`, `phase`, `reason`, `failedVersion`, `actor`, `repin` counts, `drainUntil`, and timestamps.
- `REPIN` holds the re-pin coordination state for that schema and environment (see decision 6). It exists for the authoring environment too, where no promotion item ever appears.

The request handler's single commit is a conditional `PutItem` of the `PROMOTION` item with phase `VALIDATING`, under the condition `attribute_not_exists(pk) OR phase IN (COMPLETED, FAILED)`. The item uses the public status vocabulary (`VALIDATING`, `REPLAYING`, `REPINNING`, `COMPLETED`, `FAILED`, `CANCELLED`), so the status endpoint reads the item verbatim. A failed condition returns `409` with the active `promotionId`, read through `ReturnValuesOnConditionCheckFailure: ALL_OLD`. The status read is a strongly consistent `GetItem`.

The second table, `konductor-schema-versions`, is the content store the spike proved necessary. Its key is `pk = {domain}#{eventName}#{environment}` and `sk` = version number, with `content` (as accepted by the gate), `contentHash` (SHA-256 of the canonical form), `writtenBy` (`schema-admin` or a `promotionId`), and `writtenAt`. `schema-admin` writes the authoring environment's item after each successful create or append; the promotion workflow writes the target's item after each replayed version. Canonical equality is `serde_json::Value` equality (whitespace- and key-order-insensitive), which absorbs the registry's normalization. If the record write fails after the registry write succeeded, the API returns an error so the publisher retries; the retried append hits the registry's no-change conflict, which completes the missing record instead of minting a version.

- *Why a new promotions table:* `konductor-event-streams` is keyed by producer and consumer, has a by-consumer index, and has no stream. Promotion state has a different key, a different lifecycle, and needs a stream. The registries remain the source of truth for version numbering and admission; the versions table exists because the registry cannot return historical content, not because Konductor is building a parallel catalog.
- *Why a head item instead of one item per promotion:* the head item doubles as the per-target lease without a transaction, and the spec only reads the most recent promotion.
- *Alternative:* using the Step Functions execution as the record, with `StartExecution` as the lease. Rejected: it cannot return `409` with the active identifier atomically, `DescribeExecution` is not a read model for the console, and it would break the stream-driven rubric.

### 3. The record's stream starts the workflow and emits lifecycle events

A new `schema-promotion-stream` Lambda consumes the table stream, filtered to `sk = PROMOTION`:

| Transition | Side effect |
|---|---|
| New `promotionId` with phase `VALIDATING` | `StartExecution` named `promote-{promotionId}` (an `ExecutionAlreadyExists` error counts as success) |
| `VALIDATING → REPLAYING` | `SchemaPromotionStarted` |
| `→ COMPLETED` | `SchemaPromotionCompleted` (with re-pin counts) |
| `→ FAILED` | `SchemaPromotionFailed` (with reason) |

A divergence moves `VALIDATING → FAILED` directly, so the stream emits no `Started` event. Event source mapping: bisect on error, a maximum retry age, an on-failure SQS destination, and an alarm on that destination.

Rubric answers: (1) yes, the item carries every field of the workflow input and of the event payloads; (2) yes, every side effect hangs on a committed phase transition; (3) the workflow start is idempotent by name, and lifecycle events are at-least-once with `promotionId` plus phase for consumer deduplication; (4) yes, promotion is asynchronous; (5) yes, low-volume table with a bounded consumer and an on-failure destination; (6) the workflow itself is the envelope once it starts. Both handlers get records in `docs/development/side-effects/` (a new `schemas.md` topic file).

### 4. A Standard Step Functions workflow, `konductor-promote-schema`

```
Plan ──DIVERGED──────────────────────────────────────────► MarkFailed(DIVERGED)
  │ ok
  ▼
MarkReplaying (DDB, condition promotionId)  ──►  Widen(target env, through=requested)
  ▼
ReplayLoop  (Map, MaxConcurrency 1, over fromVersion+1..requested)
  │   ReplayVersion(N) ─► UpdateItem lastWrittenVersion=N
  │   Catch → record REPLAY_FAILED, failedVersion, continue below
  ▼
EmitVersionPublished  (events:putEvents integration, Retry; skipped only if
  │                    the target still has no schema)
  ▼
MarkDraining (drainUntil) ─► Wait(DrainSeconds) ─► Narrow(to lastWritten or old pin)
  ▼
MarkCompleted | MarkFailed(REPLAY_FAILED)
```

- **Plan** (a new `schema-promotion-steps` Lambda, `action: plan`) lists versions in both registries with `ListSchemaVersions` and verifies the prefix from the versions table: for each i in 1..k (the target's latest), the target item must exist with `writtenBy` = a promotion and `contentHash` equal to the source item's hash for version i. A missing target item for the target's *latest* version is repairable in place, because the latest version is the one registry read that exists: canonical-compare `DescribeSchema` content with the source record and complete the item on a match. A missing item for any older version is unverifiable and fails closed as `DIVERGED`, as does any hash mismatch or a target ahead of its source. Plan returns the replay list or `DIVERGED` with the first differing version.
- **ReplayVersion(N)** reads the source vN record from the versions table and the target's latest version. If the latest is N−1, the step writes the recorded content with `UpdateSchema` and `ClientTokenId = {promotionId}-v{N}`, verifies the returned version is N, and writes the target's versions-table item. If the latest is already N, the step canonical-compares it against the source record: equal means an earlier attempt landed the write and the step completes the versions-table item; unequal fails the step. `ConflictException` ("no change") means the content already equals the latest version and is handled by the same comparison path. Transient errors retry with backoff; the deterministic client token makes a retried write return the originally minted version.
- Each version is a separate state, so a crash resumes at the last committed `lastWrittenVersion`. A new promotion after a failure re-runs Plan and continues from the target's latest version.
- Execution names are `promote-{promotionId}`. The execution timeout is 1 hour, and a top-level Catch writes `FAILED` so the lease is always released.
- *Alternative:* one Lambda that loops over versions. Rejected: a single long invocation loses its progress on timeout, and the drain Wait would hold a Lambda open.

### 5. Replay idempotency comes from a single writer plus read-before-write

Only the promotion workflow writes to a non-authoring registry: the authoring guard blocks the API, and the `PROMOTION` lease allows one promotion per target. Given that, "write vN only if the latest is vN−1" is a sufficient guard, including after an ambiguous failure where the write landed but the response was lost. Spike 1.1 confirmed the two service behaviors the guard relies on:

- *Identical content mints nothing:* `UpdateSchema` with content equal to the latest version fails with `ConflictException` ("no change since the previous schema version"). An accidentally repeated replay step cannot mint a duplicate version; the step treats this conflict as "already written" and verifies by canonical comparison.
- *`ClientTokenId` is idempotent:* a retry with the same token and content returns the originally minted version. Replay steps set `{promotionId}-v{N}`, so retries after ambiguous failures converge.
- *Content fidelity:* `DescribeSchema` normalizes whitespace and cannot read historical versions at all. This is why the versions table (decision 2) holds the replayable content and why every comparison is canonical-JSON, never byte-wise. The spike registry was deleted after the run.

### 6. Re-pinning: a version range per schema and environment, with generation-checked narrowing

The rule pattern is always derived from the `REPIN` item, never written blind: `schemaVersion = [floor … target] ∪ [target + 1]`. Outside a re-pin, `floor == target == pinned`, which gives the spec's `[pinned, pinned + 1]` lookahead.

- **Widen(through)** is one `UpdateItem` that sets `floor = min(existing floor, current pin)`, `target = max(target, through)`, `generation += 1`, and `drainUntil = now + DrainSeconds`, then applies the derived pattern to every affected entry.
- **Narrow(to)** is a conditional `UpdateItem` (`generation = :mine AND drainUntil <= :now`) that sets `floor = target = to`, then applies the pattern and commits `pinnedVersion` on each entry. If the condition fails, a newer re-pin owns narrowing and this one exits. After applying patterns, the actor re-reads the item. If the generation moved, it re-applies the current derived pattern, so the last writer always leaves rules consistent with the latest state.
- A partial promotion narrows `to = lastWrittenVersion`. A promotion that wrote nothing narrows back to the old pin.
- **Affected entries** are found with a `Query` on `konductor-event-streams` by `producerDomain`, filtered to the schema, for env entries that are `ACTIVE` (enabled or disabled). The workflow runs a Map (`MaxConcurrency: 10`) over them, invoking `materialize-subscription-env` in a new `mode: repin` that receives an explicit version list and a `commitPin` flag. Repin mode skips targets, alarms, and the entitlement check, and calls `DescribeRule` to pass the rule's current `State` to `PutRule`, so a re-pin can never enable a rule. A per-entry failure is recorded as `FAILED` by the function and counted, not raised.
- **Authoring appends** use a small `konductor-repin-schema-env` workflow (Widen → Wait → Narrow) started by `subscription-extender` when a `SchemaVersionPublished` event names `environments[0]`, with the execution name `repin-{domain}-{eventName}-{env}-v{version}`. Promotion events never name the authoring environment, so the two paths never overlap.
- *Why the lookahead:* an authoring append commits synchronously, and publishing stamps the new version at once. The re-pin is asynchronous. Matching `pinned + 1` delivers the new version from its first event. A matching version that does not exist yet is inert, and the append-time compatibility gate is what makes early delivery safe.
- *Alternatives:* serializing re-pins per environment through a queue (adds infrastructure, and appends would wait on each other's drain windows); a synchronous re-pin inside the append handler (breaks the one-commit rule and makes append latency grow with the subscriber count).

### 7. The drain window is a deploy parameter with a floor

`SchemaRepinDrainSeconds` defaults to `300` with `MinValue: 120`. This sits above the publish queue's retry horizon (about 90 seconds) plus batching. The workflow writes `drainUntil` to the `PROMOTION` item so the console can show the expected re-pin time.

### 8. Stages and console routing are generated from `Environments`

- **Stages:** deploy tooling renders per-environment stages. Spike 1.2 settled the mechanics:
  - `Fn::ForEach` is unusable here: environment names may contain hyphens, which are invalid in generated logical IDs and condition names, and ForEach produces map keys, not list items (so it could never touch `CacheBehaviors` anyway).
  - SAM generates its managed stage's logical ID from a literal `StageName`; an intrinsic there changes the logical ID, and updating an existing installation then fails early validation (`ResourceExistenceCheck`: the "new" stage's name already exists). The committed template therefore keeps literal `StageName` values, and `make deploy-core` rewrites the marked `StageName` lines only when the configured authoring environment differs from the committed `dev` default — a string substitution from the same validated parse that renders parameter overrides. Default installations render byte-identical templates, so the migration risk is avoided by construction rather than survived.
  - Non-first stages render into a fragment file pulled in with `AWS::Include`, matching the OpenAPI includes. Fragments must use long-form intrinsics (`Ref:`, not `!Ref`); the include transform rejects shorthand tags. For the default list the fragment emits today's logical IDs (`KonductorOAuthApiStagingStage`, `KonductorOAuthApiProdStage`), so an update from the current shape is a no-op. Verified live: a fresh stack with `["sandbox", "qa", "live"]` serves exactly `sandbox`, `qa`, `live` (plus the phantom below), and `/qa/ping` answers 200.
  - Pre-existing quirk: SAM's generated `Deployment` carries `StageName: "Stage"`, so every API already serves an undocumented phantom stage named `Stage` in production today. This change neither fixes nor worsens it; it is recorded in the decision record.
- **CloudFront:** the three per-stage behaviors become one `/api/*` behavior. A viewer-request CloudFront Function strips the `/api` prefix, so `/api/{env}/…` reaches stage `{env}`. Per-environment behaviors cannot be generated, because `CacheBehaviors` is a list and `Fn::ForEach` produces object keys, not list items. A single prefixed behavior also keeps API traffic visibly separate from SPA routes.
- **Console:** `config.json`'s `apiBaseUrl` becomes the API root (`/api`, or `{webUrl}/api` when deployed). `useApi()` takes an optional `environment` that defaults to `environments[0]`. Environment-agnostic reads use the default, and promotion and status calls pass the target. The dev-server proxy and Playwright mocks move from `/dev/**` to `/api/**`.
- *Alternative:* keeping `/{env}/*` paths and rendering the CloudFront behaviors in a pre-deploy step. Rejected: it adds a generated template artifact for no benefit over a prefix.

### 9. Authorization

The grant engine gains `schema:promote`, granted by the `publisher` role on the domain. The promotion request requires it. The status read allows `schema:promote` or any platform role, so auditors can read status but never get a promote action. Both handlers resolve the domain record first and return `404` for missing or inactive domains, as `schema-admin` does today.

### 10. Producer schema page

The page is a new route at `pages/domains/[name]/schemas/[schema].vue`. The existing `pages/domains/[name].vue` moves to `pages/domains/[name]/index.vue`, so Nuxt does not treat it as a parent layout that needs `<NuxtPage>`. It composes the existing env-agnostic schema description (per-environment versions and history), the domain's subscription listing grouped by environment, and one promotion status read per non-authoring environment. While any promotion is non-terminal, the page polls status every 5 seconds and stops on `COMPLETED` or `FAILED`. The promotion confirmation uses `NxConfirm` and its contract (Cancel first, busy blocks dismissal, focus returns to the opener). The domain Schemas panel's badges link to this page.

### 11. Recording the departure from `gate-schema-promotion`

The parked deferral planned to run the compatibility gate again at promotion. This design trusts replayed versions by exact content match instead (the confirmed decision). A new decision record, `docs/decisions/promote-schema-versions.md`, supersedes the deferral row in `deferred.md` and keeps its date and the incident context.

## Risks / Trade-offs

- **A publish DLQ redrive after narrowing drops events stamped with the old version.** → A runbook step: redrive before promoting, or accept the loss. The DLQ alarm already flags pending messages. A later change could add a "drain hold" that keeps the floor while the DLQ is non-empty.
- **Producers that stamp versions outside publish admission keep the old number and stop matching after narrowing.** → Out of scope here. The lifecycle events give `domain-system-events` the signal to warn producers.
- **A stream consumer outage leaves a `VALIDATING` lease, so every retry returns `409`.** → The on-failure destination, its alarm, and a runbook redrive. The deterministic execution name makes the redrive safe.
- **A crash between Widen and Narrow leaves rules wider than the pin.** → Wider rules only add delivery of older or not-yet-existing versions. The next re-pin or promotion converges them, and the workflow's top-level Catch still narrows on handled failures.
- **Re-pin fan-out for widely subscribed schemas is slow or throttled.** → Map concurrency of 10 with `PutRule` retry and backoff. The cost is a few seconds per hundred subscribers against a 5-minute drain.
- **Stage logical IDs change on existing installations** (for example `KonductorOAuthApiStagingStage` becomes the ForEach-generated ID). CloudFormation would create the new stage before deleting the old one and fail on the duplicate stage name. → The spike verifies this on a scratch stack. If it reproduces, the runbook documents a two-step deploy (remove the explicit stages, then add the generated ones), accepting a brief outage on pre-launch installations.
- **The `apiBaseUrl` meaning changes** (from a stage URL to the API root). → `make web-dev` and `make deploy-web` regenerate `config.json` on every run, and the config parser rejects a value that still ends in a stage segment.
- **Schemas API throttling during Plan** (`ListSchemaVersions` in two registries plus item reads). → Bounded by the version count. The step retries with backoff, and Plan runs once per promotion.
- **A crash between an append's registry write and its content record leaves an unrecorded version.** → The retried append heals it through the no-change conflict. If the record is still missing when a promotion needs it, Plan fails closed as `DIVERGED` rather than replaying unverifiable content, and the runbook's heal step is one identical re-append in the authoring environment.

## Migration Plan

1. Deploy the core stack: the table, Lambdas, the two workflows, generated stages, and the CloudFront behavior. The authoring guard ships in the same deploy. No schemas exist outside the authoring environment at launch, so nothing needs backfilling. The prefix check fails closed if that assumption is wrong.
2. Deploy the web console. `config.json` is regenerated with the new `apiBaseUrl`.
3. Add the CHANGELOG entry: **breaking**, direct schema writes on non-authoring stages now return `400`.

Rollback: redeploy the previous stack and web build. Promotion items are coordination state and can be discarded. Versions already replayed stay in their registries, which is consistent with the append-only model, and the old code treats them as ordinary versions.

## Open Questions

- Whether the 5-second polling interval on the producer schema page needs tuning once real drain times are observed.
- Whether the default `SchemaRepinDrainSeconds` of 300 should come down after measuring the publish queue's real retry distribution.
