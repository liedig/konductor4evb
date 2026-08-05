## Why

The PRFAQ promises simple `publish`, `subscribe`, and `unsubscribe` API operations that let teams produce and consume events without knowing EventBridge internals. Subscribe and unsubscribe already exist. This change adds the missing `publish` data-plane API and publisher schema management, using flat verb endpoints where the domain is in the request body rather than the URL path.

## What Changes

- Add `POST /publish` flat verb endpoint with store-and-forward durability via SQS. The caller provides domain, event name, and business data; Nexus resolves the schema, constructs the canonical envelope, and publishes to the domain's event bus for the stage-selected environment.
- Add schema management endpoints (`POST /schemas`, `POST /schemas/versions`) that mimic the EventBridge schema API. Publishers create schemas and append versions in platform-managed registries.
- Use environment-named API stages (`dev`, `staging`, `prod`) instead of a static `v1` stage. Publish and schema operations infer the target environment from `$context.stage`; the publish consumer reads it from SQS message attributes.
- Extend the canonical envelope `detail.metadata` with `idempotencyKey` (caller-optional, else generated at API Gateway time) and `publishedAt` (SQS `SentTimestamp`, immutable from queue time). Both survive EventBridge replays.
- Carry the environment through the publish flow as an SQS message attribute alongside the trusted-principal envelope.
- Require publisher authorization through the DynamoDB-backed grant engine (prerequisite: `add-grant-engine`, the standalone production grant engine change from the `evaluate-authorization-engine` spike).
- Keep schemas append-only: publishers cannot replace, delete, or retire schemas or versions.
- Validate the generated envelope but do not validate business data against schema content.
- Defer schema promotion between environments and schema-change event hooks to separate changes.

These changes add APIs without changing or removing existing API operations.

## Capabilities

### New Capabilities

- `event-publishing`: Flat store-and-forward publish API, canonical envelope construction with immutable metadata, latest-version resolution, and publisher authorization.

### Modified Capabilities

- `schema-management`: Add publisher-authorized schema creation and append-only version registration with environment inferred from the API stage.
- `event-envelope`: Extend `detail.metadata` with `idempotencyKey` and `publishedAt` for publish-time immutability and replay resilience.
- `control-plane-access`: Add publisher routes on both OAuth and IAM surfaces with environment-named stages.

## Impact

- Adds `POST /publish` with SQS store-and-forward integration on both OAuth and IAM API surfaces.
- Adds `POST /schemas` and `POST /schemas/versions` on both surfaces.
- Adds a publish consumer Lambda that reads SQS, resolves schemas, builds envelopes, and calls EventBridge `PutEvents`.
- Changes API stage naming from `v1` to environment-named stages (`dev`, `staging`, `prod`).
- Extends SQS mapping templates to inject environment and idempotency-key attributes.
- Depends on the Cognito access foundation for separate surfaces and trusted principal context, and on the production DynamoDB grant engine for publisher authorization.
