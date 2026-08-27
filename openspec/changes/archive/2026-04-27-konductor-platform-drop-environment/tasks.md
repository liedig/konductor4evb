## Preamble

This change lands after `multi-env-domain-provisioning`. By the time this change starts:

- The trust role in `trust-stack.yaml` is already renamed to `${Organization}-konductor-trust-role` and its `Environment` parameter already dropped.
- The provisioning state machine no longer uses `${Environment}` — env is sourced from the domain record per Map iteration.
- `docs/trust-stack-deployment.md` has already been updated.

What's left is strictly the Konductor platform cleanup.

## 1. Template updates

- [x] 1.1 Remove the `Environment` parameter from `konductor-core-stack.yaml`
- [x] 1.2 Remove `ENVIRONMENT` from `Globals.Function.Environment.Variables`
- [x] 1.3 Rename all `${Organization}-${Environment}-<thing>` resource names to `${Organization}-konductor-<thing>` (IAM roles, DynamoDB table, event bus, queues, log groups, API Gateway, Lambda function names, dashboards, alarms)
- [x] 1.4 Change API Gateway `StageName` from `!Ref Environment` to `v1`
- [x] 1.5 Update the API endpoint Output URL to use `v1` in place of `${Environment}`
- [x] 1.6 Update `NamespacePattern` parameter description to clarify it governs domain-resource naming only
- [x] 1.7 Remove `Environment` from the provisioning-workflow `DefinitionSubstitutions` block (already unused by the state machine after multi-env)
- [x] 1.8 Update provisioning-workflow IAM policies: references to `${Organization}-${Environment}-*` that point at Konductor-internal resources become `${Organization}-konductor-*`. References that point at domain resources (which still carry env in the name) are updated to `${Organization}-*-*` so a single policy matches any env.
- [x] 1.9 Run `sam validate --lint` and `cfn-lint` and fix any violations

## 2. samconfig

- [x] 2.1 Remove `Environment=dev` from `parameter_overrides`
- [x] 2.2 Add `AwsOrganizationArn` placeholder to the `parameter_overrides` in `samconfig-core.toml` (operator fills in their Org ARN during installation)
- [x] 2.3 Delete the `[dev.deploy.parameters]` section if nothing else distinguishes it; use `[default.deploy.parameters]` as the single environment

## 3. Makefile

- [x] 3.1 Add `AWS_REGION ?= $(shell aws configure get region 2>/dev/null || echo us-east-1)`
- [x] 3.2 Add `$(error …)` guards on required variables (`APP_ACCOUNT_ID`, `OWNER_EMAIL`) for the `smoke` target
- [x] 3.3 Add `$(error …)` guards on `FROM_EMAIL` and `TO_EMAIL` for the `deploy-notifications` target (renamed to `NOTIFICATION_SENDER_EMAIL`, `PLATFORM_OPERATOR_EMAIL` by `multi-env-domain-provisioning` 10a.1)
- [x] 3.4 Update the `smoke` target's hardcoded resource names (`acme-dev-smoke-test-*`) to reflect the current domain-resource naming for the `dev` environment (still env-scoped at domain layer)
- [x] 3.5 Verify `make deploy-core` succeeds without any `--parameter-overrides` CLI arguments, given a correctly populated samconfig

## 4. Lambda and library code

- [x] 4.1 Confirm no Rust code reads `ENVIRONMENT` (grep-verified at proposal time; re-verify after template change)
- [x] 4.2 Reserve the domain name `konductor` in the register-domain validation path so no domain can collide with the platform naming (`${Organization}-konductor-*`)
- [x] 4.3 Add a test in `domain-command-handler` covering rejection of a `konductor` domain name

## 5. Documentation

- [x] 5.1 Update `docs/installation.md`: remove `Environment` from all parameter tables, stack-deploy commands, and prerequisites
- [x] 5.2 Update `docs/development/architecture.md`: platform-vs-domain naming distinction; callout that Konductor is a singleton per org
- [x] 5.3 Add an entry to `docs/development/decisions.md` capturing the rationale for dropping `Environment` at the platform layer
- [x] 5.4 Update README "Getting Started" pointer if any stale env references remain

## 6. Verification

- [x] 6.1 `cargo check --workspace && cargo clippy --workspace -- -D warnings`
- [x] 6.2 `cargo test --workspace`
- [x] 6.3 `sam build -c` succeeds
- [x] 6.4 `sam deploy` (or `make deploy-core`) to a clean test account succeeds from an empty CloudFormation state
- [x] 6.5 Run `make smoke` to validate end-to-end

## 7. OpenSpec

- [x] 7.1 Update delta specs in `openspec/changes/konductor-platform-drop-environment/specs/` as requirements shift during implementation
- [x] 7.2 Archive this change once verified (moves delta specs into main specs)
