# Tasks: Validate at Ingest

## 1. Shared validation module

- [x] 1.1 Add the `jsonschema` crate (draft-4 support) to the workspace and create a schema-validation module in `konductor-common`; verify `cargo check` passes and the crate compiles for the Lambda target
- [x] 1.2 Implement draft-4 content parsing/validation helpers (parse check for create/append, payload validation returning machine-readable violations) with unit tests covering conformant, non-conformant, and malformed-schema inputs
- [x] 1.3 Implement the in-memory schema cache (`(environment, schema)` -> `{version, content, fetchedAt}`, TTL <= 60s) with unit tests for hit, cold miss, TTL expiry, and refresh-on-failure paths

## 2. Schema management constraints

- [x] 2.1 `schema-admin`: reject `schemaType: OpenApi3` on `POST /schemas` with `400`; verify the existing create tests pass and a new test asserts the rejection
- [x] 2.2 `schema-admin`: validate draft-4 parseability on `POST /schemas` and `POST /schemas/versions` with `400` on malformed content; verify new handler tests cover both routes
- [x] 2.3 Extend `lib/konductor-common/tests/api_surfaces.rs` contract expectations for the new `400` behaviors; verify `cargo test -p konductor-common` passes

## 3. Admission step

- [x] 3.1 Create the admission Lambda (grant-engine authorization, stage from request context, cache-backed schema resolution, payload validation, version stamping) returning synchronous `400`/`403`/`404`/`422`/`503`; verify per-binary tests pass using aws-smithy-mocks rules for DynamoDB and Schemas
- [x] 3.2 Implement refresh-before-reject: on conformance failure, refresh the cached schema once and re-validate; verify a mock test proves a stale-cache payload is admitted after refresh and a genuinely non-conformant payload still gets `422`
- [x] 3.3 Emit admitted metadata as SQS message attributes (`Environment`, `SchemaVersion`, idempotency key defaulted from the API Gateway request ID, plus the trusted-principal envelope); verify attribute presence and values in tests
- [x] 3.4 Implement fail-closed registry errors (`503`, no queueing); verify a throttling-mock test asserts the response and zero SQS calls

## 4. Delivery-only consumer

- [x] 4.1 Slim `publish-events` to read admitted attributes, construct the envelope (structural validation retained), and call `PutEvents`; verify existing retry/idempotency tests pass against the attribute-driven input
- [x] 4.2 Remove consumer-side authorization and schema resolution; verify `DescribeSchema` no longer appears in the consumer's call graph and mock tests assert zero Schemas API calls

## 5. Infrastructure and API surface

- [x] 5.1 Change the `/publish` integration on both OAuth and IAM surfaces from direct SQS to the admission Lambda (template + `api/` OpenAPI specs); verify `sam validate`/cfn-lint and `api_surfaces.rs` contract tests pass
- [x] 5.2 Add validator IAM (least privilege: grant table read, Schemas read, SQS send), reserved concurrency, log metric filter, and admission-failure alarm; verify template review shows no wildcard actions
- [x] 5.3 Audit existing registries for `OpenApi3` content before rollout and record findings; verify the audit output is attached to the PR

## 6. Docs and verification

- [x] 6.1 Update `docs/development/decisions.md` with the admission/delivery split, earned-stamp invariant, JSON-Schema-only constraint, and rejected alternatives (declared version, overlap windows); verify entries follow the existing format
- [x] 6.2 Update `docs/development/architecture.md` publish-path diagrams (API Gateway -> admission -> SQS -> delivery-only consumer); verify rendered diagrams match the implemented flow
- [x] 6.3 Run `make check` (fmt, clippy, tests) and an end-to-end `sam local` or dev-stage exercise: conformant publish returns `202` and delivers, non-conformant returns `422` with violations and queues nothing; verify CloudWatch shows zero consumer-side validation errors
