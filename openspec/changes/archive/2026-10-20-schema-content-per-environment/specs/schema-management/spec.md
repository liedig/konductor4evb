# Spec Delta

## ADDED Requirements

### Requirement: Per-environment schema content is readable

Both API surfaces SHALL provide `GET /schemas/{schemaName}/content` accepting `domain` and `environment` query parameters plus an optional `version` parameter, returning the named version's content from that environment's registry. When `version` is omitted, the environment's latest version SHALL be returned. Content SHALL be read from the environment's registry — whose per-version content is immutable — never from the version-content store, so schemas that predate content recording remain readable. Callers SHALL hold a `publisher` binding on the producer domain or a platform role, the same rule as the schema detail route, and there SHALL be no consumer-context path. A schema absent from the named environment, or a version that does not exist, SHALL return `404 Not Found`; an unauthorized caller SHALL receive `403 Forbidden`. The response SHALL identify the environment and the version returned.

#### Scenario: Publisher reads an environment's latest content

- **WHEN** a publisher bound to `sales` requests `GET /schemas/acme-sales@OrderPlaced/content?domain=sales&environment=staging` without a version
- **THEN** the response SHALL contain staging's latest version content and identify the version returned

#### Scenario: A specific version is read

- **WHEN** a publisher requests the content with `version=1` while the environment's latest is v3
- **THEN** the response SHALL contain v1's content, read from the registry's immutable per-version record

#### Scenario: Schema absent from the environment

- **WHEN** the schema exists in dev but has never been promoted to prod, and its content is requested for prod
- **THEN** the API SHALL return `404 Not Found`

#### Scenario: Unknown version

- **WHEN** the content is requested with a version the environment's registry does not hold
- **THEN** the API SHALL return `404 Not Found`

#### Scenario: Caller lacks producer access

- **WHEN** a caller with no `publisher` binding on the producer domain and no platform role requests content
- **THEN** the API SHALL return `403 Forbidden`

#### Scenario: Both surfaces enforce the rule

- **WHEN** the same content request is issued against the OAuth and IAM stages
- **THEN** both SHALL apply the identical authorization and response semantics
