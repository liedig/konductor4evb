# Platform metadata rollout — consumer-first checklist

This runbook describes how to move a Konductor for Amazon EventBridge
environment from legacy revision 0 to composed metadata contracts.
Rollout requires **separate authorization**. This checklist supports planning
and verification. It does not authorize live writes against a deployed stack.
Each step identifies the checks, responsible teams, and required evidence.

Read this before activating any environment. The architecture it describes is
in [Platform metadata composition](../architecture/metadata.md#platform-metadata-composition).
The [Decision log](../decisions/README.md) records independent versioning, the queue-time
exception, cache-cutover costs, and side-effect classifications for each handler.

## Ground rules

Apply the following rules throughout the rollout:

1. **No destructive history rewrite, anywhere.** Revisions and activation audit
   records are immutable; pointers move forward only. There is no rollback
   button because reverting a pointer would remove generated fields from new
   events—an additive-contract violation. If activation breaks consumers,
   **pause new admissions** (disable the domain or the admission path) and fix
   forward; queued events keep their recorded values and remain retrievable.
2. **Schema validation never proves external consumer tolerance.** Konductor
   validating that an event matches the composed contract says nothing about
   whether a consumer's own deserializer or validator accepts an added metadata
   field. Readiness reports and affected-schema inventories are **timestamped
   observations**; the acknowledgment step is where human owners accept
   the risk, and the activation API reruns authoritative checks without
   trusting any of it.
3. **Dual-format first, formats never guessed.** Delivery handles both the
   legacy three-field wrapper and the format-2 envelope. Absence of the
   metadata stamp means *legacy recognition*—never recovery from a malformed
   new-format message, which fails closed.
4. **Business gate boundaries stay put.** Metadata composition is not a
   business-schema version bump and does not bypass `gate-schema-promotion`;
   business tightening still rejects there, and metadata activation emits no
   business append or promotion event.

## Checklist

Complete each phase before moving to the next phase. Do not perform live writes
until the responsible operator authorizes them.

### Phase 0 — prerequisites (no service writes)

- [ ] `gate-schema-promotion` behavior is present (this rollout is blocked
      without it); record its absence explicitly rather than working around it.
- [ ] `konductor-metadata-configuration` is deployed with the conditional
      bootstrap: revision 1 (protected defaults) seeded once, and every
      existing environment pointer seeded at **legacy revision 0**. Bootstrap
      is retry-safe—redeploying must not overwrite existing pointers.
- [ ] The dual-format delivery consumer is deployed **before** any admission
      change and verified against historical fixtures. Legacy messages keep
      their original `publishedAt`/`idempotencyKey` semantics, format-2
      messages validate frozen fields from their carried descriptors, and
      neither path reads configuration at delivery.
- [ ] Baseline tests pass: admission, delivery, metadata handlers, the stream
      consumer, and the API parity suite (`make check`, focused `cargo test`).

### Phase 1 — offline inventory (still no activation)

- [ ] Compile and validate **every current business schema** offline against
      the metadata wrapper (the same offline composability check schema
      authoring enforces). Record any schema whose local references cannot
      resolve composed—those must be fixed **before** its environment
      activates; unresolvable legacy external references are a compatibility
      failure to resolve explicitly, not a fetch-at-runtime problem.
- [ ] Record the affected-schema inventory per environment from the catalog
      (business V + metadata M per schema), with its capture time. This is an
      observation for rehearsal, not a snapshot lock.
- [ ] Model pointer-read cost and latency at representative publish volume
      (the strong pointer read runs on every admission) and the activation
      cold-start registry burst. Record the numbers; throttling fails closed
      (503) and a time-to-live (TTL) pointer cache is **not** an acceptable mitigation. The
      modeled envelope and its limits are recorded in
      [Service quotas and capacity planning](service-quotas.md). Replace the assumption rows with
      measurements from the harness in the AWS Region
      (`metadata-benchmark-stack.yaml`) when a live run is authorized.

### Phase 2 — rehearsal in a sandbox

- [ ] Exercise the operator journey end to end: create a draft revision →
      preview → save (including a stale-head 409 recovery) → readiness →
      activate legacy-0 → baseline-1 in one sandbox environment → inspect the
      historical and current contract pairs.
- [ ] Exercise the race and failure drills. Cover activation races
      (expected-current conflicts) and a configuration outage after enqueue
      (delivery must not need configuration). Also cover paused delivery
      across activation, partial `PutEvents` failure, replay, and a second
      publisher retry with the same idempotency key (new universally unique
      identifier (UUID), same deduplication key).
- [ ] Verify events admitted before activation carry no metadata stamp and
      still deliver; events admitted after carry the stamped revision.

<a id="phase-3--consumer-acknowledgement-per-environment-per-consuming-team"></a>

### Phase 3 — consumer acknowledgment (per environment, per consuming team)

- [ ] Every **owning team and consuming team** touching the environment's
      events acknowledges in writing (ticket or recorded approval) that a
      metadata revision bump is a **new contract**. Consumers that validate
      the exact metadata block can reject events carrying new fields, even
      when business data is unchanged.
- [ ] The acknowledgment explicitly covers the generated fields the target
      revision adds (names, generators, constraints)—from the revision
      preview, not from memory.
- [ ] Consumers with exact-block validators state their migration: extension-
      tolerant reading, or a coordinated deploy before activation.

### Phase 4 — activation (per environment, lowest first)

- [ ] Run readiness in the administration user interface (UI) and record the observed timestamp; the
      activation API reruns authoritative checks—the UI result is never
      trusted server-side.
- [ ] Activate **one environment** (dev before prod), confirming the
      acknowledgment is on file. Monitor admission error rates (HTTP 503
      indicates configuration read failures), delivery dead-letter queue (DLQ) depth, and the first publish
      per schema (generation-keyed cache refresh).
- [ ] After activation, do **not**: deploy a consumer build that cannot read
      format 2, delete or edit revisions, or move a pointer backward. Keep the
      format-compatible build available and recorded.

### Phase 5 — fix-forward recovery (incident posture)

- [ ] If consumers break: pause new admissions (domain disable or admission
      path), let queued events drain—delivery needs no configuration and
      retries cannot drift—then fix forward: a new **additive** revision,
      or consumer-side migration. Never remove a field that shipped.
- [ ] If the stream consumer falls behind (IteratorAge alarm): bounded retry
      and durable failed-record capture are in place; permanent audit records
      support recovery beyond the 24-hour stream window—follow the runbook
      before shards block.
- [ ] If admission `503` responses persist (configuration unreadable): the
      environment fails closed by design; restoring the configuration table
      restores admission. No legacy fallback revision is ever served.

## Sign-off

Record the activation date, acknowledgment, and evidence for each environment.
The placeholders do not indicate completed activation.

| Environment | Date activated | Revision | Consumer acknowledgments | Evidence links |
|-------------|---------------|----------|---------------------------|----------------|
| dev | Not recorded | M1 | Not recorded | None |
| staging | Not recorded | M1+ | Not recorded | None |
| prod | Not recorded | M1+ | Not recorded | None |

Keep this table filled in as each environment moves; the activation audit in
`/metadata/activations` is the authoritative record, and this page is the
consumer-facing coordination trail.
