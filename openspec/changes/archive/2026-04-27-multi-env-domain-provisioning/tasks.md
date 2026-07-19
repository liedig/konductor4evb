## 0. Trust-role rename (prerequisite)

Brought forward from `nexus-platform-drop-environment` because the multi-env provisioning workflow cannot target a single application account with three env-scoped role names. See design D2.

- [x] 0.1 Rename the trust role in `trust-stack.yaml` from `${Organization}-${Environment}-nexus-trust-role` to `${Organization}-nexus-trust-role`
- [x] 0.2 Remove the `Environment` parameter from `trust-stack.yaml`
- [x] 0.3 In `trust-stack.yaml`, replace every `${Organization}-${Environment}-` prefix inside the inline IAM policy `Resource` blocks with `${Organization}-` (keeping the trailing wildcard) so one role can manage `{org}-dev-*`, `{org}-staging-*`, and `{org}-prod-*` resources across all envs
- [x] 0.4 Update the trust-role ARN in every location of `domain-management/state-machines/provisioning.asl.yaml` (AssumeRole task, each compensation step's `Credentials` block) from `'${Organization}-${Environment}-nexus-trust-role'` to `'${Organization}-nexus-trust-role'`
- [x] 0.5 Update `docs/trust-stack-deployment.md` to reflect the new role name and the removal of the `Environment` parameter
- [x] 0.6 Update `docs/installation.md` trust-stack deployment section to reflect the new role name and drop the `Environment` parameter
- [x] 0.7 Run `cfn-lint trust-stack.yaml` and fix any violations

## 1. API schema (response-only changes)

- [x] 1.1 `RegisterDomainRequest` is unchanged at the API surface — environments are platform-configured, not request-supplied. (Verified.)
- [x] 1.2 N/A (no request-body env validation needed — config-time only)
- [x] 1.3 Update `DomainDetail` (and `DomainSummary`) schemas in `api/openapi.yaml` to include `environments` and restructure `resourceArns` to the map-by-env structure (`{ [env]: PerEnvResourceArns }`)
- [x] 1.4 Update response examples in `api/openapi.yaml` to show the map-by-env `resourceArns`

## 2. Platform configuration (`Environments` SAM parameter)

- [x] 2.1 Add an `Environments` parameter to `template.yaml` (type `CommaDelimitedList`, default `dev,staging,prod`). Per-entry pattern validation happens at Lambda cold start (task 3.1) since `CommaDelimitedList` parameters do not support `AllowedPattern` in CloudFormation.
- [x] 2.2 Plumb the list into the domain command handler Lambda as an `ENVIRONMENTS` environment variable (comma-joined via `Fn::Join`)
- [x] 2.3 Document the parameter in `docs/installation.md` as an org-wide setting the platform team configures once at deploy

## 3. Domain command handler (`register.rs`)

- [x] 3.1 Parse `ENVIRONMENTS` env var at Lambda cold start into `Vec<String>`; fail fast with a clear error if missing or malformed
- [x] 3.2 On register-domain, stamp the parsed env list onto the domain record's `environments` field before persisting
- [x] 3.3 Include the stamped `environments` list in the `RegisterDomainSucceeded` event payload

## 4. Data model (`nexus-common`)

- [x] 4.1 Add `environments: Vec<String>` to the domain entity struct
- [x] 4.2 Restructure `resource_arns` from flat to `HashMap<String, PerEnvResourceArns>` (keyed by env name); the inner type holds what the current flat struct holds
- [x] 4.3 Update DynamoDB `Serialize`/`Deserialize` (attribute-value mappings) for the new structure
- [x] 4.4 Add serialisation round-trip tests against representative DynamoDB item attribute maps

## 5. Namespace resolver

- [x] 5.1 Confirm `NamespaceResolver::resolve(env, org, domain)` still produces correct names given per-env resolution (no API change expected)
- [x] 5.2 Confirm `default_pattern()` output matches the per-env resource-name expectations in the spec

## 6. State machine extraction

- [x] 6.1 Author `domain-management/state-machines/provision-env.asl.yaml` by extracting the current happy-path steps plus the inner saga from today's `provisioning.asl.yaml`. Inputs: `domainId`, `domainName`, `ownerAccountId`, `environment`. Outputs: per-env resource ARNs on success; failure-shape with failed step, error code/message, and `compensationErrors` on failure.
- [x] 6.2 Author `domain-management/state-machines/deprovision-env.asl.yaml` by lifting the compensation chain into a standalone workflow that takes a per-env resource ARN set plus `ownerAccountId` and deletes everything idempotently. Treat "resource already gone" as a successful no-op. Collect per-step failures into `compensationErrors`; do not halt on individual step failure.
- [x] 6.3 Author `domain-management/state-machines/provision-domain.asl.yaml` (new outer): set domain status to `PROVISIONING`, detect new-account, create namespace SSM parameter and RAM share at the domain level (above the Map), then a `Map` state (unbounded concurrency) over `environments` starting `provision-env.sync:2`
- [x] 6.4 In `provision-domain.asl.yaml`, on all-envs-success: update domain to `ACTIVE` with per-env resource ARN map, emit `DomainProvisioningSucceeded` with the payload described in the spec
- [x] 6.5 In `provision-domain.asl.yaml`, on any-env-failure: run a second `Map` over the envs that succeeded starting `deprovision-env.sync:2`, update domain to `PROVISIONING_FAILED`, emit `DomainProvisioningFailed` populated with `failedEnvironment`, `failedStep`, `errorCode`, `errorMessage`, and aggregated `compensationErrors`
- [x] 6.6 Delete the old `provisioning.asl.yaml` once the three replacements are wired and referenced in the SAM template

## 7. SAM template

- [x] 7.1 Define three state machines in `template.yaml` (`ProvisionDomainWorkflow`, `ProvisionEnvWorkflow`, `DeprovisionEnvWorkflow`) replacing the single `ProvisioningWorkflow`
- [x] 7.2 Grant `ProvisionDomainWorkflow` `states:StartExecution`, `states:DescribeExecution`, and `states:StopExecution` on both inner state machines (required by `.sync:2`)
- [x] 7.3 Update the provisioning EventBridge rule to target the outer `ProvisionDomainWorkflow` instead of the old `ProvisioningWorkflow`
- [x] 7.4 Update execution role IAM so inner workflows have the resource-creation permissions they need (carry over from the current provisioning role; split between inner/outer as appropriate)
- [x] 7.5 Confirm Lambda environment variables are unchanged (namespace resolver is already per-env)
- [x] 7.6 Run `sam validate --lint` and `cfn-lint template.yaml`; fix any violations
- [x] 7.7 Rename the `OrganizationArn` SAM parameter to `AwsOrganizationArn` to disambiguate it from the `Organization` identifier used in resource naming. Update `template.yaml`, `provision-domain.asl.yaml`'s `DefinitionSubstitutions`, `docs/installation.md`, and the downstream `nexus-platform-drop-environment` artefacts that referenced the old name

## 8. Notifications

- [x] 8.1 Update the approval-notification template (`send-approval-notification`) to mention that provisioning will cover the domain's configured environments (pull from the domain record)
- [x] 8.2 Confirm `send-rejection-notification` template needs no changes for this feature
- [x] 8.3 Wire a `DomainProvisioningFailed`-driven owner notification: vague content, no technical detail ("your domain encountered a problem; platform team has been notified"). Platform-ops channel routing stays out of scope.

## 9. Events

- [x] 9.1 Update the `DomainProvisioningFailed` event schema in the registry and Rust event types to include `failedEnvironment: string` (no Rust event-type structs exist; no external schema registry — event is emitted as JSON from the outer workflow, which now includes `failedEnvironment`)
- [x] 9.2 Update the outer workflow (`provision-domain.asl.yaml`) to populate `failedEnvironment` from the first failed Map iteration

## 10. Documentation

- [x] 10.1 Update `docs/development/architecture.md`: multi-env domain provisioning, three-workflow split, all-or-nothing contract, namespace/RAM persistence on retry, platform-configured env list
- [x] 10.2 Document the `Environments` SAM parameter in `docs/installation.md` (already linked from task 2.3) — ensure the smoke test section reflects the describe-domain response showing `environments` on the domain record
- [x] 10.3 Add an entry to `docs/development/decisions.md` capturing the three-workflow split, all-or-nothing contract, and platform-configured env list
- [x] 10.4 Mention in architecture docs that `deprovision-env` is the right unit for the future `DELETE /domains` flow (scope: out of this change)

## 10a. Notification refactor + email-link decide flow

Follow-up from an implementation review. Establishes two personas (Platform Operator, Domain Owner), renames notification surfaces for clarity, adds a missing approval-outcome email, and wires one-click approve/reject links via an unauthenticated GET endpoint.

- [x] 10a.1 Rename SAM parameters and env vars for clarity: `ApprovalFromEmail` → `NotificationSenderEmail`, `ApprovalToEmail` → `PlatformOperatorEmail`, `APPROVAL_FROM_EMAIL` → `NOTIFICATION_SENDER_EMAIL`, `APPROVAL_TO_EMAIL` → `PLATFORM_OPERATOR_EMAIL`. Update `template.yaml`, `notifications-stack.yaml` (also renames its `FromEmail`/`ToEmail` parameters), three Lambda sources, `Makefile` `deploy-notifications`, `samconfig.toml`, and `docs/installation.md`.
- [x] 10a.2 Rename Lambda directories for persona clarity: `send-approval-notification` → `send-approval-request-notification` (platform team inbox, request to act); `send-rejection-notification` → `send-rejection-outcome-notification` (domain owner, outcome of their request). Update workspace `Cargo.toml` members, per-crate `Cargo.toml` `name` fields, `template.yaml` CodeUri/FunctionName/LogGroup/IAM ARN references, and the `SendApprovalNotificationFunction` → `SendApprovalRequestNotificationFunction` substitution used by `approval.asl.yaml`.
- [x] 10a.3 Add unauthenticated `GET /domains/registrations/decide?decision={approve|reject}&token={taskToken}` endpoint alongside the existing IAM-authed POST. Extend the existing `decide-registration` Lambda to branch on `httpMethod` and respond with an HTML confirmation page on GET. Idempotent: `TaskDoesNotExist`/`TaskTimedOut` render a friendly "already processed" page rather than a 500. OpenAPI updated with `x-amazon-apigateway-auth: NONE` on the GET route.
- [x] 10a.4 New `send-approval-outcome-notification` Lambda, triggered by `DomainRegistrationApproved` via its own EventBridge rule (`DomainRegistrationApprovedNotificationRule`). Emails the Domain Owner that their registration was approved and provisioning is starting across the configured environments.
- [x] 10a.5 Wire approve/reject links into the request email. Pass `DECIDE_ENDPOINT_URL` (built from `NexusApi` + stage + resource path) into `send-approval-request-notification`. Email body includes two styled button links with URL-encoded task tokens (`+ / =` in SFN tokens are percent-encoded).

## 11. Verification

- [x] 11.1 `cargo check --workspace && cargo clippy --workspace -- -D warnings`
- [x] 11.2 `cargo test --workspace` (including new data-model round-trip tests and command-handler env-stamping tests)
- [x] 11.3 `sam build -c` succeeds
- [x] 11.4 Deploy to a clean test platform account with default `Environments=dev,staging,prod`; deploy the updated trust stack to a test app account
- [x] 11.5 Integration smoke: register a domain, approve it, confirm three full env sets provisioned and `resourceArns` map has three entries keyed by env
- [ ] 11.6 Redeploy platform with `Environments=dev` (single env), register another domain, confirm only one env set is provisioned
- [ ] 11.7 Failure injection (manual): break the trust policy in the app account, register a domain, confirm the outer workflow rolls back all envs and emits `DomainProvisioningFailed` with `failedEnvironment` populated

## 12. OpenSpec

- [x] 12.1 Iterate delta specs in `openspec/changes/multi-env-domain-provisioning/specs/` if the design shifts during implementation
- [x] 12.2 Archive this change once verified via `/opsx:archive`
