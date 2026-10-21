# Spec Delta

## ADDED Requirements

### Requirement: Durable subscription upgrade phase facts

Each materialized entry entering re-pinning SHALL commit self-contained start facts identifying its operation, trigger, original pin, requested version, environment, consumer, enabled state, contract selectors, and estimated completion. Terminal facts SHALL be committed atomically with the resulting pin or recorded entry failure. Public notification delivery SHALL derive from committed facts, not reconstruct history from mutable current state.

#### Scenario: Per-entry starting pins differ
- **WHEN** two subscriptions on one schema start a promotion with different recorded pins
- **THEN** each start snapshot SHALL retain that entry's original pin rather than the schema-wide minimum

#### Scenario: Notification publication fails after pin commit
- **WHEN** an entry's new pin and outcome commit but public event publication fails
- **THEN** the same committed outcome SHALL remain available to the retry path
- **AND** publication SHALL NOT rerun business-schema replay or invent a new operation identity

#### Scenario: Entry failure is committed with its outcome
- **WHEN** a narrowing operation cannot update an entry's rule
- **THEN** the recorded failure and partial-outcome facts SHALL be committed together
- **AND** failure to persist those facts SHALL propagate to the durable workflow retry/failure path rather than be silently swallowed

### Requirement: Upgrade notifications preserve re-pin behavior

Notifications SHALL preserve existing widening, drain, narrowing, enablement, and per-entry failure isolation. A successful terminal event SHALL follow the committed entry outcome, not registry publication or schema-wide narrowing intent. Entries awaiting promotion or already failed SHALL NOT generate start notifications merely because they appear in the schema's subscription inventory.

#### Scenario: Registry is ready before subscriptions finish draining
- **WHEN** the schema version has been published but an entry is still draining
- **THEN** no completed upgrade event SHALL be emitted for that entry yet

#### Scenario: Missing environment is extended instead of re-pinned
- **WHEN** a promotion materializes an environment that was awaiting promotion
- **THEN** the existing extension behavior SHALL apply
- **AND** the MVP SHALL NOT mislabel that first materialization as an upgrade from a fictitious prior version

#### Scenario: Notification delivery does not gate business progress
- **WHEN** SNS delivery or SES email is delayed or fails
- **THEN** the committed upgrade SHALL not be reversed or held waiting for acknowledgement
- **AND** publication failures SHALL remain observable independently

### Requirement: Duplicate and superseded upgrade phases

Retrying a committed phase SHALL reuse its identity and SHALL NOT advance its resource revision again. A newer re-pin SHALL supersede an older operation's uncommitted outcome. Superseded work SHALL NOT publish a misleading terminal event or overwrite newer recorded upgrade facts. A start event SHALL NOT promise a one-to-one terminal event if a later operation supersedes it.

#### Scenario: Materialization task is delivered twice
- **WHEN** a task retries after its phase facts were already committed
- **THEN** the persisted facts SHALL remain unchanged
- **AND** any repeated notification SHALL carry the original phase's idempotency key

#### Scenario: New append supersedes a draining append
- **WHEN** a v5 authoring re-pin supersedes a draining v4 re-pin
- **THEN** the v4 operation SHALL not emit a new completion claiming the entry is now at v4 after v5 has taken over
- **AND** the surviving v5 outcome SHALL describe the actual resulting pin with a higher resource revision
- **AND** consumers SHALL be able to reconcile superseded starts using the current subscription API

### Requirement: Contract selectors are snapshotted with each phase

Upgrade facts SHALL snapshot the exact metadata revision used to describe each phase's business contracts, rather than resolve the current metadata revision during projection. Metadata revision 0 SHALL remain a valid legacy selector. The event contract version SHALL be independent of both business schema versions and metadata revisions.

#### Scenario: Metadata changes during the drain
- **WHEN** metadata activation changes between an entry's start and terminal phases
- **THEN** the start facts SHALL retain their original metadata revision
- **AND** the terminal facts SHALL identify the metadata revision observed for that outcome
- **AND** replaying either fact through publication SHALL not replace its selector with the latest revision
