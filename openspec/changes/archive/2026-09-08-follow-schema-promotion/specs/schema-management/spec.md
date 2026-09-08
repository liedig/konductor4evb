# Delta: schema-management — follow-schema-promotion

## ADDED Requirements

### Requirement: Schema version publication events

The control plane SHALL emit a `SchemaVersionPublished` event on the control-plane bus (source `konductor.schema-management`) after every successful schema create or append, carrying `producerDomain`, `schemaName`, `environment` (the stage-selected environment whose registry was written), `version`, and `publishedAt`. This event is the platform's promotion signal: it is how consumers' subscriptions learn that a schema has arrived in a new environment. Emission follows the committed registry write; a failed emission SHALL be logged and SHALL NOT fail the API response, since the registry write is the committed state and appends are not safely retryable (a retry would mint a further version).

#### Scenario: Create emits version 1 for the target environment

- **WHEN** a publisher successfully registers `OrderNotification` against the staging environment
- **THEN** a `SchemaVersionPublished` event SHALL be emitted with `environment: staging` and `version: 1`

#### Scenario: Append emits the new version

- **WHEN** a publisher successfully appends version 5 of a schema in prod
- **THEN** a `SchemaVersionPublished` event SHALL be emitted with `environment: prod` and `version: 5`

#### Scenario: Emission failure does not fail the write

- **WHEN** the registry write has succeeded but emitting the event fails
- **THEN** the API response SHALL report the write's success
- **AND THEN** the emission failure SHALL be logged with the schema identity and environment
