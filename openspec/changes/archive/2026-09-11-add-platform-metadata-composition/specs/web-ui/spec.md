## ADDED Requirements

### Requirement: Platform metadata management interface

The console SHALL provide a platform metadata administration area. Administrators SHALL be able to inspect protected defaults, add generated fields and nested objects, preview a draft, review its differences from the latest revision, and save a new immutable revision. Auditors SHALL have read-only access. Domain-only users SHALL not receive configuration mutation controls, and backend authorization SHALL remain the enforcing boundary.

Each field SHALL display its type, generator, and validation semantics. Built-in and previously saved fields SHALL be locked against deletion and modification. The UI SHALL offer only supported generators, provide labeled field-level errors, preserve local edits after validation or conflict errors, and prevent duplicate submissions while busy. Existing shared controls and accessible dialog patterns SHALL be used.

#### Scenario: Administrator adds a generated field

- **WHEN** an administrator adds an `eventId` UUID field
- **THEN** the UI SHALL show its inferred type, generated-value example, validation constraints, and revision diff
- **AND THEN** saving SHALL create a revision without activating it

#### Scenario: Defaults remain protected

- **WHEN** an administrator views built-in metadata or fields inherited from a saved revision
- **THEN** the UI SHALL label them as mandatory or immutable
- **AND THEN** it SHALL not offer controls to remove or redefine them

#### Scenario: Preview has no live effects

- **WHEN** an administrator previews unsaved configuration
- **THEN** the UI SHALL show synthetic generated values and validation results for the selected environment context
- **AND THEN** preview SHALL NOT save configuration, publish an event, or move an active revision

#### Scenario: Save failure preserves work

- **WHEN** saving fails validation, authorization, or an outdated-predecessor check
- **THEN** the UI SHALL explain the failure and retain local edits
- **AND THEN** conflict recovery SHALL let the administrator reload the latest revision and review the difference

#### Scenario: Auditor inspects configuration

- **WHEN** a platform auditor opens metadata administration
- **THEN** defaults, revisions, history, and environment status SHALL be visible
- **AND THEN** save, activation, and other mutation controls SHALL be unavailable

### Requirement: Environment metadata activation interface

The console SHALL show the active metadata revision separately for each configured environment and allow administrators to explicitly select an environment for metadata activation. Revision history SHALL show definitions, authors, creation times, and activation audit records. Before activation, the UI SHALL show the target environment, current and candidate revisions, readiness results, affected current-schema inventory, and the warning that consumers must tolerate metadata additions. Readiness and inventory observations SHALL have timestamps and SHALL NOT be presented as proof of external consumer compatibility.

Activation SHALL use an explicit confirmation dialog naming the environment and revision. Busy state SHALL block dismissal and duplicate activation. Success SHALL refresh authoritative environment state; failure SHALL not optimistically show activation as complete. The environment selector is specific to metadata administration and SHALL NOT change the stage-derived target behavior of existing business-schema authoring.

#### Scenario: Activate only staging

- **WHEN** an administrator confirms activation of M2 for staging
- **THEN** the UI SHALL submit an activation for staging with the expected current revision
- **AND THEN** after success it SHALL show staging at M2 without changing dev or prod

#### Scenario: Activation races another administrator

- **WHEN** the server returns a stale-current-revision conflict
- **THEN** the UI SHALL reload authoritative activation state and require review before another submission
- **AND THEN** it SHALL not silently overwrite the other administrator's activation

### Requirement: Schema contract inspection in the console

Domain schema and catalog detail views SHALL show business versions and metadata revisions independently per environment. On the catalog detail view, each environment indicator SHALL be a selectable link that loads that environment's current contract, and the view SHALL present the raw business schema ("Schema content") and the complete composed document ("Schema preview") side by side, both resolved from the same selected environment. The Schema preview SHALL be captioned as the in-force contract that events in the selected environment validate against, identifying the business version, metadata revision, and definition digest. The console SHALL NOT expose historical pair selectors at this stage; historical inspection remains an API capability via the documented explicit `schemaVersion`/`metadataRevision` selectors. Producer schema editors SHALL keep metadata read-only and SHALL provide an effective-contract preview without requiring producers to author envelope fields, labelled as provisional and unregistered so it cannot be confused with the in-force Schema preview.

#### Scenario: Current schema reflects metadata activation

- **WHEN** staging moves from M1 to M2 while its business schema remains V3
- **THEN** the refreshed schema view SHALL display V3/M2 and the added metadata
- **AND THEN** it SHALL not claim that a business schema version was appended

#### Scenario: Environment selection drives both panels

- **WHEN** a reader selects a different environment indicator on the catalog detail view
- **THEN** the Schema content and Schema preview panels SHALL reload from that environment's current contract
- **AND THEN** the displayed business version and metadata revision SHALL be that environment's active pair

#### Scenario: In-force preview is not a provisional preview

- **WHEN** a reader views the catalog Schema preview panel
- **THEN** it SHALL be identified as the contract events in the selected environment currently validate against
- **AND THEN** the producer authoring preview SHALL remain labelled as unregistered and distinct from it

#### Scenario: Existing journeys retain behavior

- **WHEN** a publisher authors business content or a subscriber inspects a subscription
- **THEN** metadata inspection SHALL not alter business-schema append rules, entitlement checks, or subscription version pins
