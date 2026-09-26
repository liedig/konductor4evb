# Tasks

## 1. Shared model and API contract

- [x] 1.1 Add `environmentAccounts` (optional, sparse) to the domain record serde shape in `konductor-common` and implement the shared resolution/validation helpers (`resolve account = map[env] ?? ownerAccountId`; map keys must be members of the platform environment list; values must be 12-digit account IDs). Verify: unit tests cover default resolution, sparse override, duplicate accounts, unknown-key rejection, and malformed-value rejection; verify the record shape tolerates the absent field on old records and old code tolerates the new field (design rollback risk).
- [x] 1.2 Add the optional `environmentAccounts` map to the register-domain request schema in `api/openapi-oauth.yaml` and `api/openapi-iam.yaml` (values constrained to the 12-digit pattern structurally; key membership is handler-side). Verify: the update-domain schema still omits the field; `api_surfaces` and template tests pass.

## 2. Registration and read paths

- [x] 2.1 In `register.rs`, validate the map against the platform-configured environment list, store it sparse on the record, and build the effective map into the approval-workflow input and the `RegisterDomainSucceeded` detail; failure reasons name the offending key or value. Verify: handler tests (smithy-mocks) cover all six scenarios of the "Per-environment account overrides" requirement, including byte-identical behavior for a mapless registration.
- [x] 2.2 Expose the effective map on the read paths the console and retry need: `describe-domain` and `pending-registrations` responses resolve it via the shared helper, and `domain-state` builds `DomainProvisioningRetryRequested` carrying it. Verify: handler tests assert the resolved map in each response/event; `list-domains` is unchanged.

## 3. Workflows and notifications

- [x] 3.1 Carry the effective map through `approval.asl.yaml` into `DomainRegistrationApproved` and `DomainRegistrationRejected`, and render the map (one line per environment, env → account) in the approval-request notification email. Verify: notification body construction test (priority flow) shows the map; events match the delta payloads.
- [x] 3.2 In `provision-domain.asl.yaml`, resolve each environment's account from the approved event's effective map and invoke the inner workflow with the resolved `accountId`; keep new-account detection querying the `by-owner` GSI with the default account only. Verify: `sam build` and `cfn-lint` pass.
- [x] 3.3 Rename the inner workflows' input from `ownerAccountId` to the resolved `accountId` at all eight construction sites (`provision-env` ×6, `deprovision-env` ×2), including the PutEvents resource-policy Principal. Verify: `sam build` and `cfn-lint` pass; no remaining `ownerAccountId` reference in either file.
- [x] 3.4 In `retag-domain.asl.yaml`, resolve the account per environment from the record and iterate distinct accounts so a shared account is retagged exactly once. Verify: `sam build` and `cfn-lint` pass; per-account dedup covered by a focused test where the harness allows, otherwise by recorded inspection evidence in the task note. **Note (inspection evidence, design D6):** every tag target is an environment-scoped resource from `resourceArns`, so iteration stays per environment and a shared account can never be tagged twice for the same resource — the account only scopes the trust-role credentials. This is recorded as an inline comment on `RetagEnvironments`. The delta wording ("iterate the distinct accounts") is reconciled to match this reality in task 5.1.

## 4. Console

- [x] 4.1 Add a `KonductorEnvironments` stack output (joined platform `Environments`) and write it into `web/public/config.json` in `deploy-web` and `web-dev`; expose it via `useKonductorConfig()`. Verify: `pnpm typecheck` and `pnpm build` pass; the generated config carries the list.
- [x] 4.2 Add optional per-environment account override fields to the domain-request form (one per configured environment, empty means default account, 12-digit format check), and render the effective env → account map on the approval card and domain detail view. Verify: Playwright coverage per AGENTS.md (role/label locators, viewport per test, exact toast assertions); `pnpm lint`, `pnpm typecheck`, `pnpm build` pass.
- [x] 4.3 Add a `web-ui` spec delta to this change covering the override form and the effective-map presentation. Verify: `openspec validate per-environment-domain-accounts --strict` passes with the delta included.

## 5. Documentation, records, and final validation

- [x] 5.1 Update operator and reference docs: trust-stack runbook deploys once per distinct account used; `service-quotas.md` counts environments resolving into an account (not domains) for per-account headroom; registration field documentation and the API contract and data-model reference pages gain `environmentAccounts`. Verify: documentation validation shows no broken links or anchors.
- [x] 5.2 Record the decision in `docs/decisions/per-environment-domain-accounts.md` (indexed, preserving the 2026-09-24 decision date) and add a `CHANGELOG.md` Unreleased entry linking to it; claim no deployment or acceptance evidence. Verify: links resolve.
- [x] 5.3 Final gates: `openspec validate per-environment-domain-accounts --strict`, `make check`, `cargo test --workspace`, `sam build` plus `cfn-lint`, the web gates, and the markdown checks all pass. Verify: no live AWS operations were performed without separate authorization.
