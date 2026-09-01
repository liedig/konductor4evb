# Tasks: editable-cost-center

Work test-first on the priority flows per AGENTS.md: domain update lifecycle and shared-library correctness are priority flows; write failing tests before each handler/change.

## 1. API contracts

- [x] 1.1 Add `costCenter` (`type: string`, `minLength: 1`, `maxLength: 256`, description mirroring the registration field's) to `UpdateDomainRequest` in `api/openapi-oauth.yaml` and `api/openapi-iam.yaml`; verify `additionalProperties: false` remains and both specs still lint (the repo's API spec check in `make check` or CI).
- [x] 1.2 Update the `UpdateDomainSucceeded` event contract in `docs/domain-management.md` (detail gains `requestId`, and `costCenter` when changed); verify the documented example matches what `update.rs` emits.

## 2. Update handler (domain-registrations)

- [x] 2.1 Add `cost_center: Option<String>` to `UpdateDomainBody` in `domain-management/functions/domain-registrations/src/update.rs` (camelCase, `deny_unknown_fields` unchanged); add a failing test that an update carrying `costCenter` deserializes and one that an unknown field still fails; verify with `rtk cargo test -p domain-registrations`.
- [x] 2.2 Validate `costCenter` via `konductor_common::tags::validate_cost_center`, emitting `UpdateDomainFailed` with the reason and no DynamoDB write on failure; cover with a mocked test asserting zero `update_item` calls; verify green.
- [x] 2.3 Include `costCenter` in the "no fields provided" check; test that a body of only `costCenter` is accepted and an empty body still emits the no-fields failure; verify green.
- [x] 2.4 Extend `build_update_expression` to set `costCenter` and recompute `tags` via `konductor_common::tags::effective_tags(org, domain, None, Some(cc))` in the same `UpdateItem` (read `ORGANIZATION` env like `register.rs`); test asserts both attributes appear in the update expression values and `costCenter` is in `changedFields`; verify green.
- [x] 2.5 Thread the SQS `messageId` from `handler.rs` into `update::handle` as `request_id` and add `requestId` plus `costCenter` (when changed) to the `emit_success` detail; extend the success-event test to match on both; verify green.
- [x] 2.6 Run `rtk cargo clippy -p domain-registrations -- -D warnings` and `rtk cargo fmt --check`; verify clean.

## 3. Retag workflow (domain-management/state-machines/retag-domain.asl.yaml)

- [x] 3.1 Create `retag-domain.asl.yaml` (YAML, JSONata, STANDARD): LoadDomain (`aws-sdk:dynamodb:getItem` for `ownerAccountId`, `environments`, `resourceArns`), TagNamespaceParameter (`ssm:addTagsToResource` with the record's `costCenter`), then an INLINE Map over environments; verify `sam validate` (or `make build`-time template validation) accepts the definition.
- [x] 3.2 Per environment, add platform-account tag states mirroring provision-env retry blocks: `events:tagResource` (platform bus), `schemas:tagResource` (registry), `sqs:tagQueue` (subscription DLQ, QueueUrl derived from ARN), `cloudwatch:tagResource` (dashboard plus the two alarm ARNs derived from the `{org}-{env}-{domain}-*` naming convention, with a comment marking the coupling); verify tags come from the loaded record's `costCenter`, not the event.
- [x] 3.3 Add owner-account tag states with `Credentials: { RoleArn: arn:aws:iam::{ownerAccountId}:role/{org}-konductor-trust-role }`: `events:tagResource` (app bus) and `sqs:tagQueue` (integration DLQ); verify against trust-stack.yaml that no trust-role policy change is needed (both actions already allowed).
- [x] 3.4 Add terminal failure path emitting `RetagDomainFailed` (`domainName`, `requestId`, `failedEnvironment`, `failedStep`, `errorCode`, `errorMessage`) on the control-plane bus, with no record mutation and no compensation; verify every task state's Catch routes there.

## 4. Infrastructure (konductor-core-stack.yaml)

- [x] 4.1 Add `RetagWorkflowRole` + policy: `dynamodb:GetItem` on the domains table; tag-only actions (`events:TagResource`, `schemas:TagResource`, `sqs:TagQueue`, `cloudwatch:TagResource`, `ssm:AddTagsToResource`) scoped to `{Organization}-*`; `events:PutEvents` on the control bus; `sts:AssumeRole` on `arn:aws:iam::*:role/{org}-konductor-trust-role` (same cross-account pattern as the provisioning policy); verify `sam validate` and confirm the role holds no create/delete permissions.
- [x] 4.2 Add the `RetagDomainWorkflow` state machine with an `Events: EventBridgeRule` on the control bus matching `source: konductor.domain-management`, `detail-type: UpdateDomainSucceeded`, `detail.changedFields` contains `costCenter`, with the standard rule-failure DLQ; the rule passes the full event as input and executions are auto-named (rule targets cannot set execution names — duplicate executions are tolerated because retag calls are idempotent upserts); verify `rtk aws cloudformation validate-template` / `sam validate` passes.
- [x] 4.3 Add the state machine ARN to stack outputs alongside the other workflow ARNs; verify `sam build --config-file samconfig-core.toml -c` succeeds.

## 5. Web console

- [x] 5.1 Add `costCenter?: string` to `UpdateDomainRequest` in `web/app/types/api.ts`; verify `pnpm nuxi typecheck` passes in `web/`.
- [x] 5.2 In `web/app/pages/domains/[name].vue`, add a cost-center `NxInput` to the edit modal, pre-fill from the domain's `costCenter` (falling back to the `cost-center` entry in the rendered `tags` map if the API doesn't surface it — confirm against `describe-domain` render and, if missing, add `costCenter` to `render_domain`), include it in `saveEdit`, replace the "cost center is not editable" hint with one noting name/account/environments remain read-only, and client-side reject an empty value; verify typecheck and `pnpm lint`.
- [x] 5.3 Manual smoke: `make web-dev` against the deployed dev stack, edit a domain's cost center, confirm 202, `UpdateDomainSucceeded` with `costCenter` in `changedFields`, describe rendering the new tag, and the retag workflow execution completing; verify the new `cost-center` value on one platform-account resource and one owner-account resource via the console links.

## 6. Docs and spec hygiene

- [x] 6.1 Record the retag-path side-effects classification (single commit in `update.rs`, retag driven by the committed-transition event, idempotent tag upserts tolerating duplicate executions) in `docs/development/decisions.md` per the AGENTS.md rubric; verify the entry follows the existing audit format.
- [x] 6.2 Update `docs/domain-management.md` update-domain examples to show `costCenter`; verify rendered JSON matches the API schema.
