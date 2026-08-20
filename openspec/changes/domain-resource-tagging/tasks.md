# Tasks: domain resource tagging

## 1. Tag model (konductor-common + registration)

- [x] 1.1 `effective_tags` helper in konductor-common: standard set from org/domain/env + optional `cost-center`; single source for every caller
- [x] 1.2 Registration contract: `RegisterDomainRequest.tags` (map) → `costCenter` (optional string, validated ≤256 chars, no control chars); record persists `costCenter` and the computed effective tag set
- [x] 1.3 Reject the legacy `tags` field and any custom key other than `costCenter` with 400 (API schema + handler tests)
- [x] 1.4 Update path rejects `costCenter` (immutable in v1); describe/list render the effective set from the record

## 2. Provisioning saga tagging

- [x] 2.1 Platform-side steps (platform event bus, schema registry, subscription DLQ, dashboard, alarms) pass tags at creation via the shared helper
- [x] 2.2 App-side steps (app event bus, integration DLQ via trust role) pass the same tags
- [x] 2.3 Domain-level SSM namespace parameter tagged with the env-less set
- [x] 2.4 Saga test: tagging failure in an env step fails and compensates that environment

## 3. Trust stack

- [ ] 3.1 `konductor-trust-role` policy gains scoped `events:TagResource`/`UntagResource` and `sqs:TagQueue`/`UntagQueue` on `{org}-*`
- [ ] 3.2 Operator docs note the trust-stack redeploy requirement

## 4. Console

- [x] 4.1 Request form: replace the key/value tag rows with a single optional Cost center field
- [x] 4.2 Domain detail: render the effective tag set (standard + cost-center distinctly marked); edit form drops tag editing

## 5. Gates and verification

- [x] 5.1 `make check`, workspace tests green, `openspec validate domain-resource-tagging --strict`
- [x] 5.2 Deploy core + trust stack (demo account), register a domain with a cost center, approve, verify tags on resources in both accounts and in the console
