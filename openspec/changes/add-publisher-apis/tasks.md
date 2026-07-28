## 1. Production Publisher Authorization

- [ ] 1.1 Consume the completed `evaluate-authorization-engine` readiness decision and add the DynamoDB grant engine's production resources, configuration, and adapter behind a storage-neutral publisher decision and binding interface
- [ ] 1.2 Add typed publisher-binding models and validation for issuer-qualified Cognito team groups and twelve-digit AWS accounts, including stable identifiers, domain scope, trusted actor metadata, and rejection of global publisher bindings
- [ ] 1.3 Add decision tests for trusted OAuth group membership, trusted IAM account identity, OAuth/IAM parity, principal-type isolation, cross-domain denial, additive overlapping bindings, malformed context, and fail-closed engine errors
- [ ] 1.4 Implement durable publisher-binding persistence and current-state decisions through the DynamoDB grant engine, including required cache invalidation and service-restart behavior
- [ ] 1.5 Add OAuth `platform-admin` handler tests and implementations for create, list, and revoke publisher bindings, including idempotency, non-admin denial, publisher self-administration denial, conflict handling, and token-free audit records

## 2. Dual-Surface API and Shared Contracts

- [ ] 2.1 Add strict request and response models for binding administration, schema creation, schema version append, and event publication, with tests proving callers cannot supply trusted principal, canonical identity, schema version, registry, bus, or envelope metadata
- [ ] 2.2 Add equivalent schema and publish route contracts to the separate OAuth and IAM control-plane contract sources, and add publisher-binding administration only to the OAuth platform-admin surface
- [ ] 2.3 Configure each OAuth route for the access-foundation REQUEST authorizer and each IAM route for AWS_IAM, then add contract tests proving credentials are not interchangeable and normalized principals reach shared handlers
- [ ] 2.4 Add cross-surface contract tests for identical publisher requests, responses, resource actions, error mappings, and `400`/`403`/`404`/`409`/downstream failure behavior
- [ ] 2.5 Add shared domain/environment target derivation and event-name validation tests, then implement canonical schema name, registry, bus, source, and detail-type derivation
- [ ] 2.6 Add canonical envelope construction and validation tests, then implement numeric latest-version stamping, `metadata`/`data` structure validation, serialization checks, and EventBridge entry-limit checks without business-payload schema validation

## 3. Publisher Schema Operations

- [ ] 3.1 Add handler tests for schema creation on normalized OAuth and IAM principals covering authorization-before-resource-access, decision parity, active domain and environment validation, canonical target derivation, version 1 creation, duplicate conflict, and AWS error mapping
- [ ] 3.2 Implement the focused synchronous schema creation Lambda shared by both control-plane surfaces against AWS Schemas with dependency-injected clients and structured operational metrics
- [ ] 3.3 Add handler tests for version append on normalized OAuth and IAM principals covering authorization-before-resource-access, decision parity, canonical target derivation, immutable prior versions, missing schema, concurrent version allocation behavior, and AWS error mapping
- [ ] 3.4 Implement the focused synchronous schema version Lambda shared by both surfaces using the AWS Schemas version-producing operation without exposing replacement, deletion, retirement, or implicit schema creation

## 4. Synchronous Event Publishing

- [ ] 4.1 Add publish handler tests for normalized OAuth and IAM principals covering authorization-before-lookup, decision parity, active domain and environment validation, latest selected-environment schema resolution, absent schema, and rejection of caller-supplied identity or version fields
- [ ] 4.2 Add publish handler tests covering canonical envelope output, numeric version stamping, pass-through business data without schema validation, envelope validation failure before EventBridge, accepted entries, request-level failures, and per-entry failures
- [ ] 4.3 Implement the dedicated synchronous publish Lambda shared by both surfaces with injected domain-state, authorization, Schemas, and EventBridge clients; return `202 Accepted` only after `PutEvents` accepts the entry
- [ ] 4.4 Add structured publish metrics and logs for authentication mechanism, authorization denial, schema-resolution latency/failure, EventBridge acceptance/failure, and end-to-end latency without logging business payloads, group memberships, or sensitive claims

## 5. Infrastructure and Least Privilege

- [ ] 5.1 Register the production authorization adapter and new Lambda crates in the Rust workspace, then add SAM resources, configuration, API Gateway Lambda permissions, and synchronous integrations for both control-plane surfaces
- [ ] 5.2 Grant publisher operation functions only their required domain-state, authorization-decision, Schemas, and EventBridge actions scoped to Nexus-managed resource patterns
- [ ] 5.3 Grant binding-administration functions only DynamoDB binding create/list/revoke and audit permissions, and enforce OAuth `platform-admin` route access
- [ ] 5.4 Update provisioning and managed access policies so application teams cannot directly administer registries or perform generic schema mutation, while retaining platform-admin registry ownership
- [ ] 5.5 Add infrastructure assertions for OAuth/IAM route parity, distinct authorizers, binding administration isolation, grant-engine persistence, Lambda integrations, publisher IAM allowlists, and absence of publisher registry/bus/policy/rule administration permissions

## 6. Verification and Documentation

- [ ] 6.1 Run focused crate tests during each red/green/refactor cycle, then run `make check` and `make test` for the complete workspace
- [ ] 6.2 Validate the SAM template and both control-plane API contracts, including binding administration, OAuth/IAM publisher parity, successful schema creation, append, publication, and all specified error responses
- [ ] 6.3 Run production-adapter integration tests proving durable create/list/revoke, audit attribution, restart persistence, current-state revocation, overlapping binding behavior, default denial, and OAuth/IAM decision parity
- [ ] 6.4 Update `docs/development/decisions.md` with the production DynamoDB authorization adapter, publisher binding model, dual-surface parity, dedicated synchronous publish Lambda, latest-visible-version semantics, envelope-only validation, and append-only schema rationale
- [ ] 6.5 Update `docs/development/architecture.md` with publisher-binding administration and persistence, OAuth/IAM publisher routes, normalized principal decision flow, synchronous schema and publish paths, resource ownership boundaries, and deployed Lambda/IAM resources
