## Why

Application teams need a supported, domain-scoped path to register event contracts and publish conforming events without managing EventBridge registries, buses, or envelope conventions directly. Publisher APIs make those operations self-service while preserving Nexus ownership of topology, identity derivation, and authorization.

## What Changes

- Add domain- and environment-scoped APIs for publishers to create schemas and append immutable schema versions in registries managed by the platform.
- Add a synchronous event publish API that accepts an event name and business data, resolves the latest schema version, constructs the canonical Nexus envelope, and publishes to the selected domain bus.
- Expose publisher operations on the separate OAuth and IAM control-plane surfaces where caller compatibility requires them; normalize each authenticated caller and apply identical resource-authorization decisions.
- Add production platform-admin APIs and persistence for Nexus `publisher` resource-role bindings from Cognito team-group and AWS-account principals to one or more domains through the DynamoDB-backed grant engine.
- Keep `publisher` resource-scoped rather than a global Cognito role, and keep registries, buses, policies, rules, subscriber grants, and authorization administration outside publisher control.
- Keep schemas append-only: publishers cannot replace historical versions, delete schemas, or retire schemas.
- Validate the generated event envelope but do not validate the business payload against its schema.
- Defer schema promotion between environments to a separate change.

These changes add APIs without changing or removing existing API operations.

## Capabilities

### New Capabilities

- `event-publishing`: Domain-scoped synchronous publishing, publisher authorization, canonical envelope construction, latest-version resolution, and EventBridge delivery behavior.

### Modified Capabilities

- `schema-management`: Add publisher-authorized schema creation and append-only version registration for a selected domain and environment.
- `event-envelope`: Move event-name constraint enforcement to in-platform schema registration and require the publish API to derive and validate canonical envelope fields.
- `control-plane-access`: Extend the Cognito/IAM foundation with dual-surface publisher routes, trusted team-group and AWS-account principal normalization, and platform-admin management of persistent domain publisher bindings.

## Impact

- Adds matching authenticated route contracts to the separate OAuth and IAM control-plane surfaces for schema writes and synchronous event publishing, plus OAuth platform-admin binding administration.
- Adds a dedicated synchronous publish Lambda with access to domain metadata, AWS Schemas, and domain EventBridge buses.
- Adds production binding administration, persistence, decision evaluation, audit, and revocation integration through the DynamoDB-backed grant engine validated by the prerequisite spike.
- Extends schema-management handlers and IAM permissions for publisher-authorized writes while retaining platform ownership of registries.
- Depends on the Cognito access foundation for separate surfaces and trusted principal context, and on the authorization-engine spike for validated production readiness and SLOs; this change implements the publisher binding slice rather than assuming another change supplies it.
