## 1. Binding Storage

- [x] 1.1 Add `nexus-authorization-bindings` DynamoDB table to the core stack with the RoleBinding/AuditRecord key schema, admin GSI (`DOMAIN#{domain}` → `ROLE#{role}#PRINCIPAL#{principal}`), and deletion/retention policy matching existing tables
- [x] 1.2 Add typed binding models in `nexus-common`: typed principal (issuer-qualified Cognito team group or twelve-digit AWS account), role (`publisher`/`subscriber`), domain scope, state, creation metadata, trusted actor; reject global (domain-less) bindings at the model level
- [x] 1.3 Add grant-table access helpers in `nexus-common` (key construction, get/put/delete, conditional expressions, GSI query) with unit tests against the documented key patterns

## 2. Grant Engine

- [x] 2.1 Add decision-parity test fixtures seeded from the spike's frozen fixture manifest (`evaluate-authorization-engine/evidence/fixture-manifest.json`), covering OAuth group binding, AWS-account binding, cross-domain denial, additive overlapping bindings, malformed principal context, and default denial
- [x] 2.2 Implement `InMemoryGrantEngine` behind `ResourceAuthorizer` with fixture-fed bindings; prove the parity suite passes
- [x] 2.3 Implement `DynamoGrantEngine` behind `ResourceAuthorizer` with strongly consistent reads, fail-closed error mapping, and structured internal deny-reason logging (no tokens, claims, or payloads in logs); prove the parity suite passes identically against a DynamoDB-backed test harness
- [x] 2.4 Add centralized action-string constants in `nexus-common` for the actions the engine recognizes, so call-site typos surface in tests

## 3. Authorizer Renames

- [x] 3.1 Rename `FailClosedAuthorizer` → `DenyAllAuthorizer` and `FoundationAuthorizer` → `IamOnlyAuthorizer` in `nexus-common` and all consumers; behavior and trait signatures unchanged
- [x] 3.2 Update doc comments to describe behavior and the bridge nature of `IamOnlyAuthorizer` (deleted when subscribe paths move to the engine)

## 4. Binding Administration API

- [x] 4.1 Add strict request/response models for create (`domain`, `role`, typed `principal`), list (by domain), and revoke (by bindingId); tests prove callers cannot supply audit fields, binding identifiers, or global bindings
- [x] 4.2 Add `POST /bindings`, `GET /bindings?domain=…`, and `DELETE /bindings/{bindingId}` routes to the OAuth OpenAPI contract only, wired to the REQUEST authorizer; add contract tests proving the routes reject IAM credentials and non-admin OAuth callers with 403
- [x] 4.3 Implement the binding administration Lambda: platform-admin gate, idempotent create via conditional put (`attribute_not_exists`), durable revoke via conditional delete, 404/409 error mapping, and one `TransactWriteItems` commit per mutation carrying its token-free AuditRecord
- [x] 4.4 Add handler tests for create/list/revoke, idempotent re-create, cross-surface rejection, non-admin denial, concurrent-mutation conflict mapping, and audit record contents

## 5. Development Ergonomics

- [x] 5.1 Add a seed helper (script or `make seed-bindings`) that creates developer bindings for configured test Cognito groups and AWS accounts through the admin API or direct table writes
- [x] 5.2 Document the bootstrap path in `docs/development/oauth-authentication-guide.md`: platform-admin resolves outside the grant table, so first bindings can always be created

## 6. Latency Validation

- [ ] 6.1 Add an in-region benchmark harness (Lambda or EC2 in the production Region) reusing the spike's declared measurement procedure
- [ ] 6.2 Measure decision p50/p99 against the spike's recommended SLOs (`schema:publish`-class p50 ≤ 5 ms / p99 ≤ 20 ms) and record results as change evidence; if missed, record which remediation-ladder step applies

## 7. Infrastructure and Least Privilege

- [ ] 7.1 Grant the binding administration Lambda only conditional put/delete, GSI query, and transactional write on the bindings table
- [ ] 7.2 Reserve read-only bindings-table permissions for future publisher consumers (no consumer wiring in this change)
- [ ] 7.3 Add infrastructure assertions for table schema, GSI projection, route authorizer configuration, and absence of IAM-surface binding routes

## 8. Verification and Documentation

- [ ] 8.1 Run focused crate tests during each red/green/refactor cycle, then `make check` and `make test` for the workspace
- [ ] 8.2 Validate the SAM template and OAuth API contract, including the three binding routes and all specified error responses
- [ ] 8.3 Update `docs/development/decisions.md` with the two-store split, stringly-typed trait retention, dropped deny-as-unknown, no revocation worker rationale, conditional-write/atomic-audit design, and authorizer renames
- [ ] 8.4 Update `docs/development/architecture.md` with the bindings table, engine implementations, binding administration flow, and the dark-ship wiring model
- [ ] 8.5 Update `add-publisher-apis` tasks to consume `DynamoGrantEngine`, removing its production authorization-adapter prerequisite sections
