# Design

## Context

See [proposal.md](proposal.md) for motivation and the selected MVP boundary.

Observed implementation:

- `event-streams/state-machines/promote-schema.asl.yaml` validates the target prefix, widens rules, replays versions, drains, and narrows. `repin-schema-env.asl.yaml` handles authoring appends with the same re-pin coordinator.
- `schema-promotion-steps/src/repin/{widen,narrow}.rs` maintains a schema/environment `REPIN` item with a monotonic generation. It knows the subscription inventory, but schema-level narrowing happens before individual rule/pin commits.
- `materialize-subscription-env/src/repin.rs` changes the rule and commits `pinnedVersion`. Its present best-effort failure write is insufficient for a durable public outcome. `Outcome.enabled` also comes from the task input; public events must reflect current committed/observed state instead.
- `schema-promotion-stream` emits aggregate promotion lifecycle events from the promotions-table stream. It deliberately ignores `REPIN` items and does not have per-entry phase facts.
- `EventStreamsTable` has no stream configured. Stream records contain an environment map but no public upgrade snapshot. `merge_env_stream` replaces an entire environment sub-map, so notification history must not be silently erased by repair.
- Domain provisioning and retagging already distinguish environment resources from the domain-wide namespace. `ownerAccountId` and `environmentAccounts` are immutable through normal domain updates.
- Domain detail already returns domain-level identifiers separately from environment resources. Both OpenAPI surfaces and the handwritten UI types must gain the same optional field.
- Existing owner notifications provide SES rendering and error-classification patterns, but there are no promotion-specific owner email handlers.

This is a cross-cutting design because the public contract depends on persistence, workflow ordering, account isolation, resource lifecycle, and deployment order.

## Goals / Non-Goals

**Goals**

- Describe committed per-entry outcomes without making SNS or SES part of the promotion's success condition.
- Reuse the existing re-pin generation, DynamoDB stream patterns, compatibility checker, and domain detail page.
- Ship a usable setup path for existing domains as well as new registrations.
- Keep the state needed for deterministic publication bounded per subscription/environment.

**Non-Goals**

- No additional public lifecycle families, producer/owner audience emissions, structural schema diffs, notice period, or automated code changes.
- No replay API, archive replay runbook, historical subscription catch-up, delivery history UI, SNS subscription-management API, or HTTPS/chat adapters.
- No generic event ledger, workflow framework, exactly-once promise, or migration of unrelated handler-side lifecycle emissions.
- No renaming of existing infrastructure or redesign of business-schema promotion.

## Decisions

### 1. Project committed entry snapshots directly from the subscription table stream

Add optional top-level `upgradeEvents`, keyed by environment, to each event-stream record. Each value holds the most recently committed upgrade phase and all facts needed to project it. Keep it outside `environments.<env>` so repair/materialization does not erase it. Preserve it through subscription writes and serialization round-trips; legacy records without it remain valid.

Enable `NEW_AND_OLD_IMAGES` on `EventStreamsTable`. A new `system-event-projector` compares old/new phase identities for every changed environment. It ignores REMOVE records, unchanged snapshots, and ordinary subscription mutations. The projector validates a typed snapshot and emits the public event without reading the latest subscription, schema, or metadata state.

The stream's immutable sequence of old/new images preserves successive phases even when the row now contains a later snapshot. This avoids inventing a general outbox table or writing a second copy of each event. No new internal EventBridge phase event is needed: the committed snapshot is the internal input to the public projection. Existing internal lifecycle events keep their contracts.

**Alternative:** Derive per-consumer notifications from aggregate `SchemaPromotionStarted/Completed`. Rejected because those payloads lack individual original pins, metadata selectors, enablement, and authoring outcomes.

### 2. Bind phase facts to the existing durable re-pin workflow

Extend the materialization input with operation ID, trigger, generation, original entry pin, requested target, phase timestamps, and selector snapshots. Carry the exact per-entry original pin, not the schema-wide `oldPin` minimum.

For the start phase, commit a self-contained snapshot after validation and generation allocation, before applying that entry's widened rule. For the terminal phase, apply the narrowed rule and atomically update the recorded pin plus terminal snapshot in one DynamoDB write. On per-entry failure, commit the failure status plus partial snapshot together. Replace the narrow-phase best-effort failure write with a required, retriable commit. Workflow catch paths must persist any entry outcome they report rather than fabricate success.

These calls remain envelope-driven: Step Functions already supplies durable retry context for the rule mutation and commit. There is one synchronous DynamoDB commit per phase; no handler calls SNS, SES, or public `PutEvents` after that commit. A crash after a rule change but before commit retries the same intended pattern and phase. Public delivery failure cannot undo business state.

Keep existing generation guards and drain semantics. Snapshot conditions prevent an older phase from overwriting a newer snapshot; a superseded workflow does not fabricate a terminal event. Preserve the existing no-op handling when no entries are eligible. Before a pin/failure commit, check that the entry is still eligible and the operation is current; subscription removal or a newer re-pin must not be undone by notification bookkeeping.

Use a deployment setting `SystemEventEmissionEnabled`, initially false, to gate new phase-snapshot production while leaving topic provisioning and delivery infrastructure installed. Pass the setting through the existing platform configuration/deployment path. After backfill and readiness checks, operators enable it without changing public contracts. Disabling it stops new facts; already committed facts must still drain through the projector.

**Alternative:** Publish from `repin.rs` immediately after `UpdateItem`. Rejected because a process failure between the write and publication would permanently lose the notification.

### 3. Derive identity and ordering without another counter service

Use the existing schema/environment re-pin generation `g` to derive `resourceRevision`: `2*g - 1` for start and `2*g` for terminal. Revisions are compared only for `(subscriptionId, environment)` and describe published upgrade states, not every unrelated subscription edit. Gaps are valid. `REPIN` generation must remain monotonic and must not be reset as part of repair or subscription re-creation.

The idempotency key is a versioned deterministic hash of organization, subscription ID, environment, operation ID, and phase. The operation ID is the existing promotion ID or deterministic authoring execution name; it is also the `correlationId`. Persist `publishedAt` once as the logical phase-publication timestamp when committing the snapshot, rather than sampling a new time during each send.

On a conditional-write conflict, reread: the same phase identity is success with the original snapshot, a newer revision is a superseded no-op, and a genuine competing/error state is surfaced. A newer start can arrive before an older terminal; the older terminal must not regress recorded facts. An earlier start can therefore be superseded without a terminal partner. Clients track current state by resource revision, not by counting start/end pairs.

**Alternative:** Use wall-clock timestamps or EventBridge IDs. Rejected because neither provides retry-stable identity and reliable per-resource revision ordering.

### 4. Define a small typed public contract

Check in JSON Schema draft-4 definitions under `api/system-events/` and typed models in `konductor-common`. The SNS message contains the full EventBridge event; callers parse the public `detail` rather than treating transport `id` as identity.

| Location | Meaning |
|---|---|
| `source`, `detail-type` | `konductor.system` and one of the two upgrade types |
| `detail.metadata.schemaVersion` | Public event contract version, initially 1 |
| `idempotencyKey`, `publishedAt`, `correlationId`, `resourceRevision` | Stable values from the committed phase |
| `detail.data.audience`, `domain`, `environment` | `CONSUMER`, recipient domain name, concrete upgrade environment |
| `resource` | `{ "type": "SUBSCRIPTION", "id": "<consumer>.<producer>.<detailType>" }` |
| `producerDomain`, `schemaName`, `trigger` | Producer, canonical registry schema name, `PROMOTION` or `AUTHORING` |
| `fromVersion`, `requestedVersion`, `resultingVersion` | Positive business versions; resulting is null at start |
| `intermediateVersions` | Null or an inclusive `{ "fromVersion": n, "toVersion": m }` range strictly between original and requested versions; never includes the lookahead slot |
| `enabled`, `subscriptionStatus`, `expectedRepinAt` | State observed for the phase and an estimate, not a scheduling promise |
| `contracts` | `from`, `requested`, and nullable `resulting` selectors |
| Terminal-only `outcome`, `reason` | `COMPLETED` or `PARTIAL`; safe `REPLAY_PARTIAL` or `REPIN_FAILED` reason for partial outcomes |

Each contract selector contains the producer domain, canonical schema name, environment, business `schemaVersion`, and `metadataRevision`. The event's recipient supplies `consumerDomain` when calling the existing `/schemas/{schemaName}/contract` endpoint. Start selectors use the metadata revision snapshotted at start; the resulting selector uses the revision observed at the terminal commit. Retain the original from/requested selectors in the terminal snapshot. These describe exact composed contracts, not a promise that all in-flight business events used that metadata revision.

Read the authoritative metadata activation state, including valid revision 0; do not substitute latest metadata during projection. A requested target selector can be unavailable until replay completes. Schema and entitlement errors from the existing contract API remain real errors, not new access granted by an event.

An individual entry failure produces `PARTIAL`, the actually recorded pin, `subscriptionStatus: FAILED`, and a safe reason, without claiming the rule is delivering normally. A successful partial replay reports the last written pin. Raw AWS errors, task tokens, owner emails, and schema documents are never public payload fields.

SNS attributes are the seven mandatory fields listed in the spec plus optional terminal `outcome`, keeping raw SQS delivery below SNS's ten-attribute limit. Use String attributes except Number for the public `schemaVersion`; body and attribute values must agree. Future null environments map to the attribute string `ALL`; current upgrade events always have a concrete environment.

**Alternative:** Include schema documents/diffs or an array proportional to every intermediate version. Rejected for MVP scope and unbounded payload growth; selectors plus a compact range are sufficient.

### 5. Register contracts without opening another authoring surface

Provision `{org}-konductor-system-schemas` and register one schema per event type. Reuse the custom-resource/bootstrap pattern of `metadata-bootstrap` for a small deployment-time system-schema bootstrap that validates definitions, reads the current registered version, and invokes the existing compatibility checker before appending.

A checked-in manifest declares the intended contract version and canonical definition hash. Identical content/version is a no-op; unexpected registry history or an unprovable/incompatible update fails deployment. Validate and register before enabling a projector that emits that version. Registry versions and public contract versions must agree; drift must not silently create a different version. The schema's `metadata.schemaVersion` field accepts positive integers rather than a version-specific constant that would itself make every later version incompatible.

Distribute the canonical checked-in definitions by links from the setup/reference guide; no new schema-discovery API or domain registry read grant is required. Business schema authoring/promotion APIs never target this registry.

**Alternative:** Treat Rust structs as the only contract or trust CloudFormation's unconstrained schema updates. Rejected because consumers need discoverable definitions and a checked compatibility boundary.

### 6. Route one recipient-scoped event to one verified topic

Create the system bus, 90-day archive, and one routing rule in the core stack. The rule matches `konductor.system` and the two known detail types, invoking `system-event-router`. The projector already produces a separate event for each affected subscription, so the router needs no domain census or generalized multi-recipient fan-out.

The router loads the recipient domain, accepts retained infrastructure in `ACTIVE` or `INACTIVE`, and verifies `systemEventsTopicArn` matches the expected topic name, partition, platform account, and region. A missing or inconsistent ARN is a failure, not permission to publish to an inferred or payload-supplied destination. Publish the full event with filter attributes. Restrict its IAM permissions to domain reads and the managed topic ARN pattern; only the projector can publish to the system bus.

Use at-least-once delivery. Do not add a sent-message ledger: recording success before send loses events, while recording after send still permits duplicates. Document durable consumer-side idempotency before external actions and resource revision checks before updating local state.

After a successful publish, the router stamps a latest-event marker (`systemEventsLastEventAt`, `systemEventsLastEventType`) on the domain record with a newer-wins conditional write. The marker exists solely to drive the console's unread indicator: it carries no history and its failure is logged, never retried through republication.

**Alternative:** One EventBridge rule per domain, or publishing to application buses. Rejected because SNS gives endpoint-owned subscriptions without consuming a bus rule per domain or mixing business and system contracts.

### 7. Restrict subscriptions by account and protocol

The topic policy grants only `sns:Subscribe` to the deduplicated union of owner and effective environment accounts. Create protocol-specific statements: `sns:Protocol = sqs` with `sns:Endpoint` constrained to SQS ARNs in the registered account set, equivalent Lambda restrictions, and an `sns:Protocol = email` statement. Email addresses are not ARNs, so `sns:Endpoint` cannot constrain them: any registered account may subscribe an arbitrary email address. This is an accepted trade-off (user decision, 2026-10-21): the payload is a low-sensitivity version notification, and HTTPS/SMS/chat remain denied. Keep standard topic management restricted to platform roles. Principals still need their own IAM grants and endpoint-owner resource permissions.

Provisioning subscribes the domain owner email by default so the notification path exists before teams wire SQS or Lambda endpoints. SNS email subscriptions start pending confirmation; the owner must confirm the address, and an unconfirmed subscription is a normal state, not a provisioning error. A committed owner-email change unsubscribes the old address and subscribes the new one through the update path.

This is account-level isolation, not Cognito-team isolation. Domains sharing an AWS account share that account's administrative trust boundary; topic discovery does not change it. Cross-account SQS and Lambda setup guides must show source-topic-scoped queue/function permissions, subscription ownership, filter configuration, and endpoint delivery-failure handling. Teams own endpoint DLQs and permissions.

**Alternative:** Keep SQS/Lambda-only delivery, or permit HTTPS/chat and arbitrary endpoints. SQS/Lambda-only was superseded by the user's request for email delivery with a provisioned owner default; HTTPS/chat remain rejected to bound the delivery and data-disclosure surface.

### 8. Add a domain-wide topic lifecycle and reusable ensure helper

Introduce a focused domain-system-topic helper for idempotent SNS ensure/verify/policy/tag/delete operations. It performs SNS effects only; workflows own domain-record commits. Use deterministic names and verify existing ownership tags before changing an existing topic. Never take over a colliding unowned topic.

The outer `provision-domain` workflow creates/configures the topic once, before environment provisioning, and subscribes the domain owner email as the default endpoint. Its final success update stores top-level `systemEventsTopicArn`; its compensation chain deletes the managed topic (subscriptions die with it), reports real cleanup errors, and preserves existing namespace/RAM behavior. Extend provisioning IAM permissions and failure paths, including zero successful environments.

A committed owner-email change reconciles the default subscription: unsubscribe the old address when present and subscribe the new one. Reconciliation failure stays observable without reversing the domain metadata update, mirroring cost-center retag semantics. The backfill ensures the owner email subscription too, so existing domains converge to the same default.

Add an operator script following the repository's existing backfill-script pattern. It pages through `ACTIVE` and `INACTIVE` domains, invokes the same ensure helper, and conditionally records the ARN while preserving lifecycle status and all other resources. Read current domain metadata before policy/tag reconciliation; normal account maps are immutable. A failed domain is reported and retryable; completed domains and existing endpoint subscriptions are preserved. Provide dry-run and per-domain selection as well as all-eligible operation. Backfill does not emit historical upgrade events.

The script is a repeatable administrative operation, not a claim of exactly-once provisioning. If it stops after SNS succeeds but before the database write, rerunning verifies the topic and completes that single write. Do not put backfill inside a CloudFormation custom resource that scans the fleet and risks stack-wide timeout/rollback.

Retag the recorded topic in `retag-domain.asl.yaml` using platform credentials when cost center changes; absence during rollout is a no-op. Explicit domain teardown tooling must also remove the managed topic safely, while ordinary disable retains it. Do not add a new domain-deletion API.

**Alternative:** Re-provision existing domains or assume an empty installation. Rejected because the user selected non-destructive backfill.

### 9. Separate machine delivery, human email, and failure recovery

Consumer emails subscribe to the public system bus's two event types with `trigger=PROMOTION`, independently of SNS topic delivery. A producer failure handler consumes existing `SchemaPromotionFailed` on the internal bus. Reuse current owner-recipient lookup, HTML escaping, and SES permanent/transient classification. All new SES handlers/rules use the existing optional-notification condition; system infrastructure does not.

Check `PutEvents` per-entry failures, not just transport success. For the DynamoDB projector, configure bounded retries, partial-batch failure reporting/bisection, an iterator-age alarm, and an on-failure **S3 destination retaining the full invocation payload for 90 days**. An SQS-only DynamoDB failure destination contains stream references and is not sufficient recovery after the 24-hour stream expires. Use an explicit event-source mapping resource if SAM's shorthand cannot express the required S3 destination.

Restrict and encrypt the failure bucket, block public access, and alarm on failures. Persisted failure payloads support targeted failed-message redrive with unchanged identities, not arbitrary history replay. EventBridge target-invocation DLQs and Lambda processing-failure destinations are separate paths and both need coverage; use shared system queues with supported 14-day SQS retention and alarms. Consumer endpoint delivery failures remain the endpoint owners' responsibility.

The EventBridge archive is audit-only for this MVP. It is not a substitute for failure queues, and there is no archive replay procedure or automatic historical catch-u### 10. Expose discovery without introducing administration

Add optional `systemEventsTopicArn` to both OpenAPI `DomainDetail` schemas, the existing describe handler, and the UI type; the marker fields `systemEventsLastEventAt`/`systemEventsLastEventType` join both `DomainDetail` and `DomainSummary` (the marker is not sensitive like the ARN, and the console indicator must not require one describe call per domain). Omit these for records without the data; do not change domain visibility or immutable update fields.

The console gets a top-level System events navigation item with a dedicated icon rather than an embedded domain-page card (user decision, 2026-10-21). The page lists visible domains with copyable topic ARNs and setup guidance. The unread indicator compares the newest visible marker against a per-browser `localStorage` timestamp written on page visit; there is deliberately no event feed, no server-side read state, no topic mutation, subscription UI, or replay action.

**Alternative (superseded):** An embedded domain-page card, and later a full event-history or notification-inbox page. The card was replaced by the user's top-level-navigation request; a feed remains rejected because it would require per-domain event persistence that the push-only MVP deliberately avoids.link to the setup guide. No AWS management console link, environment selector, topic mutation, subscription UI, or replay action. Reuse the existing API error/not-found behavior.

**Alternative:** Build a full event-history or subscription-management page. Rejected because ARN discovery and a tested setup guide are enough for the MVP.

## Risks / Trade-offs

- **Stream retention and poison records** → Full-payload S3 failure retention, bounded retries, partial-batch behavior, iterator-age alarms, and an operational failure-redrive test.
- **Rule side effects cannot be atomic with DynamoDB** → Keep existing durable workflow context, retry idempotently, and publish only committed facts. Do not promise atomic AWS rule/pin updates or fix unrelated workflow races under this change.
- **Overlapping upgrades do not guarantee paired start/end messages** → Monotonic generation-derived revisions and explicit supersession semantics; current-state APIs provide reconciliation.
- **Snapshot added to a mutable stream model** → Preserve it through every write/repair round-trip, test removed/recreated subscriptions, and ensure legacy records remain readable.
- **Platform-wide snapshot/metadata dependencies** → Keep the payload small and independent of schema documents; surface failures before claiming a phase committed. An operationally failed workflow is visible, not a silent notification drop.
- **Cross-account SNS policy mistakes** → Test both principal and endpoint account restrictions, default/override accounts, email allowance, remaining unsupported protocols (HTTPS/SMS), and same-account cases with least-privilege endpoint policies.
- **Owner email subscription lifecycle** → Email subscriptions stay pending until the address owner confirms; treat pending as normal, reconcile on committed owner change, and let teardown's topic deletion remove subscriptions rather than tracking subscription ARNs on the domain record.
- **Provisioning or rollout interruption** → Deterministic managed-topic verification, no adoption of unrelated topics, resumable per-domain backfill, and no destructive rollback of existing domains.
- **Existing documentation conflicts** → Record rather than silently repair unrelated claims. `docs/architecture/schemas.md` says the registry serves only latest despite versioned reads; existing naming specs use `{org}-konductor-*` while the deployed template uses unprefixed `konductor-*`; project context asks for 30-day domain DLQs although SQS supports at most 14 days. The new system resources use this proposal's names; no old names change, and these shared system queues do not pretend to satisfy a 30-day domain-DLQ promise.
- **Email ordering/duplication and schema access changes** → Explicit at-least-once limits, phase-labelled emails, and unchanged contract authorization. SNS receipt is not a credential.
- **Scale and quotas** → One router rule rather than 1,000; one topic per domain; paginated backfill and existing workflow concurrency bounds. Validate topic/policy and account quotas before rollout rather than claiming unmeasured capacity.

## Migration Plan

1. Implement and validate offline. Deploy no resources as part of planning.
2. Deploy schema definitions/bootstrap, system bus/archive, failure destinations, routing, topic helper/provisioning changes, and the optional API/UI field. Create the subscription stream and projector mapping before enabling any new snapshot writes, so committed phases cannot fall into an unmapped interval.
3. Run the backfill in dry-run mode, then apply it under explicit operator authorization to `ACTIVE` and `INACTIVE` domains. Verify ownership, policies, tags, recorded ARNs, and a zero-failure summary. New provisioning uses the shared-topic step.
4. Set `SystemEventEmissionEnabled=true` only after topic coverage, schema bootstrap, stream mapping, and failure alarms are ready. Set the event-source starting position to `TRIM_HORIZON`; legacy records have no snapshots and emit nothing.
5. With separately authorized test resources, verify one promotion and one authoring append, a paused subscription, a partial outcome, SQS/Lambda policy isolation, and duplicate delivery. Verify failed-message recovery retains the original identity. Do not test archive replay.
6. Roll back producers before disabling projection. Keep new fields, topics, subscriptions, schemas, and archives; old readers ignore optional data. Drain outstanding deliveries or retain them in failure storage before removing mappings. Never undo committed pins, delete existing domain resources, or reset re-pin generations as a rollback shortcut.

No product-scope questions remain. The user selected existing-domain backfill, selectors without diffs, and an audit-only archive for the MVP. Delivery was initially SQS/Lambda-only; the user later extended it (2026-10-21) to include email endpoints with a provisioned owner-email default that follows committed owner changes.
