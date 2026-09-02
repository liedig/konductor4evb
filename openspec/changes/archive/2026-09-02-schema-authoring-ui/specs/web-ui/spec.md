## ADDED Requirements

### Requirement: Schema authoring journeys

The producer's domain page schemas panel SHALL offer a "Register schema" action to callers with a `publisher` binding on that domain. The form SHALL accept an event name and JSON Schema draft-4 content; `schemaType` SHALL be fixed to `JSONSchemaDraft4` and SHALL NOT be selectable. The form SHALL display the target environment, derived from the console's configured API stage, and SHALL NOT offer an environment selector. Each listed schema SHALL offer an "append version" action with the same content editor. Content SHALL be validated as parseable JSON client-side before submission. Success SHALL report the schema name and resulting version and SHALL refresh the panel's per-environment version badges. Failure responses SHALL be surfaced inline — `409 Conflict` for a duplicate schema, `403 Forbidden` for missing authorization, `400 Bad Request` with the server's validation detail — and SHALL NOT clear the form's entered content. Callers without a `publisher` binding SHALL see no authoring affordances.

#### Scenario: Publisher registers a schema

- **WHEN** a publisher submits the register form with an event name and valid draft-4 content
- **THEN** the console SHALL create the schema and display its canonical name and version 1
- **AND THEN** the panel SHALL refresh to show the new schema with its environment badge

#### Scenario: Publisher appends a version

- **WHEN** a publisher submits new content via a schema's append-version action
- **THEN** the console SHALL append the version and display the new version number
- **AND THEN** the panel's version badge for the target environment SHALL update

#### Scenario: Malformed JSON caught before submission

- **WHEN** the content editor holds text that is not parseable JSON
- **THEN** the console SHALL block submission and identify the parse error
- **AND THEN** no API call SHALL be made

#### Scenario: Server validation failure is surfaced and content is retained

- **WHEN** the API rejects a submission with `400 Bad Request` (for example, content that is not a valid draft-4 document)
- **THEN** the console SHALL display the server's validation detail inline
- **AND THEN** the form SHALL retain the entered event name and content

#### Scenario: Duplicate schema surfaces conflict

- **WHEN** the API returns `409 Conflict` because the derived schema name already exists in the stage-selected registry
- **THEN** the console SHALL display the conflict inline
- **AND THEN** the form SHALL retain the entered content

#### Scenario: Target environment is shown, not chosen

- **WHEN** a publisher opens either authoring form
- **THEN** the form SHALL display the target environment derived from the console's API stage
- **AND THEN** no environment input SHALL be offered

#### Scenario: Non-publisher sees no authoring affordances

- **WHEN** a caller without a `publisher` binding on the domain views the domain page
- **THEN** the register and append-version actions SHALL NOT be rendered
- **AND THEN** the read-only schema list behavior SHALL remain unchanged
