# Capability: Schema Management

## Purpose

TBD - created by syncing change add-event-subscriptions. Update Purpose after archive.

## Requirements

### Requirement: Schema discovery

The control plane SHALL provide read-only schema discovery so consumers can find event contracts to subscribe to. `GET /schemas?domain={producerDomain}` SHALL list the schemas in the producer's registries, merged across environments and keyed by schema name, reporting the latest version per environment. `GET /schemas/{schemaName}?domain={producerDomain}` SHALL return schema detail including per-environment latest versions and the schema content for the latest version. Discovery SHALL read from the per-environment schema registries directly (source of truth for contracts) — no separate schema table SHALL be introduced.

#### Scenario: List a domain's schemas

- **WHEN** a caller requests `GET /schemas?domain=orders`
- **THEN** the response SHALL list each schema in the `orders` registries (e.g., `prezzee-orders@OrderNotification`) with the latest version per environment

#### Scenario: Version skew visible before subscribing

- **WHEN** a schema's latest version differs across environments (e.g., dev: 5, staging: 3, prod: 3)
- **THEN** the discovery response SHALL report each environment's latest version distinctly, so consumers see skew before signing the contract

#### Scenario: Missing schema or domain

- **WHEN** a caller requests schemas for a domain that does not exist or is not `ACTIVE`, or a schema name absent from every environment
- **THEN** the API SHALL return `404 Not Found` with an error message identifying the missing entity

### Requirement: Discovery is read-only in this capability version

The schema-management capability SHALL NOT provide schema registration, mutation, versioning, or deletion operations in this version. Domain owners register schemas in their per-domain registries out-of-band via the AWS Schemas API, as permitted by the registry access policies created at provisioning. Any authenticated platform caller MAY discover any active domain's schemas — visibility restriction (RBAC) is explicitly deferred.

#### Scenario: No write operations exposed

- **WHEN** a caller attempts to create, update, or delete a schema through the Nexus API
- **THEN** no such endpoint SHALL exist in the OpenAPI contract

#### Scenario: Discovery open to authenticated callers

- **WHEN** any IAM-authenticated caller requests schema discovery for an active domain
- **THEN** the request SHALL be served regardless of the caller's relationship to the domain
