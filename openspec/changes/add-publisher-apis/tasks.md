## 1. API Stage Migration

- [x] 1.1 Update SAM template to change API Gateway stage naming from `v1` to environment-named stages (`dev`, `staging`, `prod`) on both OAuth and IAM API surfaces
- [x] 1.2 Verify existing control-plane operations (register, approve, subscribe, list, describe) work identically across all environment stages
- [x] 1.3 Update OpenAPI specs (`openapi-oauth.yaml`, `openapi-iam.yaml`) to reflect the stage-based URL structure

## 2. Publish API Contract and SQS Integration

- [ ] 2.1 Add `PublishRequest` model (`domain`, `eventName`, `data`, optional `idempotencyKey`) to nexus-common with validation and tests rejecting caller-supplied identity, version, registry, bus, or envelope fields
- [ ] 2.2 Add `POST /publish` route to both OAuth and IAM OpenAPI contracts with SQS integration and `202 Accepted` response returning `requestId`
- [ ] 2.3 Write VTL mapping template for publish: inject `Environment` from `$context.stage`, inject `IdempotencyKey` (caller-supplied or `$context.requestId`), inject trusted-principal envelope attributes, push caller body to SQS
- [ ] 2.4 Add contract tests proving OAuth and IAM publish routes have identical request/response semantics and that credentials are not interchangeable

## 3. Publish Consumer Lambda

- [ ] 3.1 Add consumer handler tests for principal parsing from SQS attributes, publisher authorization before resource access, missing/malformed principal rejection, and cross-domain denial
- [ ] 3.2 Add consumer handler tests for canonical envelope construction: schema name derivation, latest-version resolution, `detail.metadata` with `schemaVersion`, `idempotencyKey`, `publishedAt`, and `detail.data` pass-through
- [ ] 3.3 Add consumer handler tests for `publishedAt` as ISO 8601 conversion of SQS `SentTimestamp`, immutability across retries, and `idempotencyKey` from message attribute
- [ ] 3.4 Add consumer handler tests for PutEvents success (delete from queue), PutEvents failure (SQS retry with same attributes), absent schema (404 equivalent), and inactive domain rejection
- [ ] 3.5 Implement the publish consumer Lambda with dependency-injected domain-state, authorization, Schemas, and EventBridge clients; read environment from SQS message attributes to select registry and bus
- [ ] 3.6 Add structured publish metrics and logs for auth mechanism, authorization denial, schema-resolution latency/failure, EventBridge acceptance/failure, and end-to-end latency without logging business payloads or sensitive claims

## 4. Schema Management API

- [ ] 4.1 Add `CreateSchemaRequest` and `AppendSchemaVersionRequest` models with validation, tests rejecting caller-supplied schema name, version, registry, or envelope fields
- [ ] 4.2 Add `POST /schemas` route to both OpenAPI contracts with synchronous Lambda integration; handler tests for authorization, canonical name derivation, version 1 creation, duplicate conflict (409), and inactive domain (404)
- [ ] 4.3 Implement the schema creation Lambda with environment from `$context.stage`, AWS Schemas client, and structured metrics
- [ ] 4.4 Add `POST /schemas/versions` route to both OpenAPI contracts with synchronous Lambda integration; handler tests for authorization, canonical name derivation, next-version creation, missing schema (404), and immutable prior versions
- [ ] 4.5 Implement the schema version append Lambda using AWS Schemas version-producing operation without exposing replacement, deletion, or retirement
- [ ] 4.6 Add event-name validation at schema creation: character constraints (`[A-Za-z0-9._-]`) and rule-name length feasibility within EventBridge's 64-character limit

## 5. Envelope Model Extension

- [x] 5.1 Add `idempotencyKey` and `publishedAt` fields to the envelope `metadata` model in nexus-common with serde and validation
- [x] 5.2 Add envelope construction tests proving `metadata` contains exactly `schemaVersion`, `idempotencyKey`, `publishedAt` and `data` contains the caller payload
- [x] 5.3 Add envelope validation tests for numeric positive schema version, non-empty idempotency key, valid ISO 8601 timestamp, JSON serialization, and EventBridge entry limits

## 6. Authorization Integration

- [ ] 6.1 Integrate publish consumer and schema handlers with the `DynamoGrantEngine` decision interface delivered by `add-grant-engine` (prerequisite: `add-grant-engine` complete)
- [ ] 6.2 Add authorization tests for OAuth team-group binding, IAM AWS-account binding, OAuth/IAM parity, cross-domain denial, missing binding denial, and fail-closed engine errors
- [ ] 6.3 Verify publisher role cannot access registry, bus, policy, rule, subscriber-grant, or authorization-administration operations

## 7. Infrastructure and Least Privilege

- [ ] 7.1 Register the publish consumer Lambda crate in the Rust workspace and add SAM resources (Lambda, SQS event source, API Gateway integrations, IAM role)
- [ ] 7.2 Grant publish consumer only domain-state read, authorization-decision (attach the reserved `nexus-bindings-table-read` managed policy from `add-grant-engine`), Schemas read, and EventBridge `PutEvents` scoped to Nexus-managed resource patterns
- [ ] 7.3 Grant schema handlers only domain-state read, authorization-decision (the reserved `nexus-bindings-table-read` managed policy), and Schemas write scoped to Nexus-managed registry patterns
- [ ] 7.4 Add infrastructure assertions for OAuth/IAM route parity, distinct authorizers, stage naming, SQS integration, and absence of publisher administration permissions

## 8. Verification and Documentation

- [ ] 8.1 Run focused crate tests during each red/green/refactor cycle, then run `make check` and `make test` for the complete workspace
- [ ] 8.2 Validate the SAM template and both control-plane API contracts, including publish, schema creation, version append, and all specified error responses
- [ ] 8.3 Exercise publish durability end-to-end: verify SQS retry on PutEvents failure preserves same idempotency key and published timestamp
- [ ] 8.4 Update `docs/development/decisions.md` with flat verb endpoints, store-and-forward rationale, environment stage inference, envelope metadata immutability, and auth engine separation
- [ ] 8.5 Update `docs/development/architecture.md` with publish data-plane flow, schema management paths, environment stage routing, envelope metadata fields, and resource ownership boundaries
