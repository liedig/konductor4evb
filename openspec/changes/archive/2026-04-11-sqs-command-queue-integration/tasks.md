## 1. SAM Template: SQS Queues and IAM

- [x] 1.1 Add `DomainsCommandQueue` (SQS) and `DomainsCommandDlq` (SQS) resources to `template.yaml`
- [x] 1.2 Add `RegistrationsCommandQueue` (SQS) and `RegistrationsCommandDlq` (SQS) resources to `template.yaml`
- [x] 1.3 Add `ControlPlaneApiSqsRole` IAM role with `sqs:SendMessage` on both command queues and `sqs:GetQueueAttributes` for API Gateway
- [x] 1.4 Add `DomainsCommandQueueUrl` and `RegistrationsCommandQueueUrl` as SAM Globals environment variables so command handler Lambdas can reference them

## 2. OpenAPI: Replace Write Integrations

- [x] 2.1 Add `x-amazon-apigateway-request-validators` block and `x-amazon-apigateway-request-validator: "Validate body"` to all write operations
- [x] 2.2 Replace `POST /domains` integration: change from `aws_proxy` Lambda to `aws` SQS service integration with VTL template (body only, no path params needed); response `202 {"message":"OK"}`
- [x] 2.3 Replace `PUT /domains/{domainId}` integration: `aws` SQS service integration with VTL that wraps body and injects `domainId` path param; `HttpMethod` attribute set to `PUT`
- [x] 2.4 Replace `POST /domains/registrations/{requestId}/decision` integration: `aws` SQS service integration with VTL that wraps body and injects `requestId`; `HttpMethod` attribute set to `POST`
- [x] 2.5 Update write operation response schemas: change `201` to `202` on `RegisterDomain`; update all write response bodies to `{"message":"OK"}`; remove `RegistrationRequestResponse` and `DomainDetail` from write responses

## 3. Create domain-command-handler Lambda

- [x] 3.1 Scaffold `domain-management/functions/domain-command-handler/` with `Cargo.toml` and `src/main.rs`
- [x] 3.2 Implement SQS event entry point: iterate records, extract `HttpMethod` message attribute, dispatch to `register` or `update` handler module
- [x] 3.3 Implement `register` module: port business logic from `register-domain/src/main.rs` (validation, DynamoDB writes, Step Functions start); emit `RegisterDomainSucceeded` or `RegisterDomainFailed` to control plane bus
- [x] 3.4 Implement `update` module: port business logic from `update-domain/src/main.rs` (fetch, validate, DynamoDB update, deactivation); emit `UpdateDomainSucceeded`, `UpdateDomainFailed`, or `DomainDeactivated` as appropriate
- [x] 3.5 Return error for unrecognised `HttpMethod` to trigger SQS retry/DLQ
- [x] 3.6 Add `DomainCommandHandlerFunction` to `template.yaml` with SQS ESM trigger on `DomainsCommandQueue`, appropriate IAM policies (DynamoDB, EventBridge, SFN), and log group

## 4. Create registration-command-handler Lambda

- [x] 4.1 Scaffold `domain-management/functions/registration-command-handler/` with `Cargo.toml` and `src/main.rs`
- [x] 4.2 Implement SQS event entry point: extract `HttpMethod`, dispatch to `decide` handler module
- [x] 4.3 Implement `decide` module: port business logic from `approve-registration/src/main.rs` (fetch request, validate state, SFN callback); emit `DecideRegistrationSucceeded` or `DecideRegistrationFailed`
- [x] 4.4 Add `RegistrationCommandHandlerFunction` to `template.yaml` with SQS ESM trigger on `RegistrationsCommandQueue`, IAM policies (DynamoDB, SFN, EventBridge), and log group

## 5. Remove Old Write Lambdas

- [x] 5.1 Remove `RegisterDomainFunction`, `RegisterDomainLogGroup` from `template.yaml`
- [x] 5.2 Remove `UpdateDomainFunction`, `UpdateDomainLogGroup` from `template.yaml`
- [x] 5.3 Remove `ApproveDomainRegistrationFunction`, `ApproveDomainRegistrationLogGroup` from `template.yaml`
- [x] 5.4 Remove `register-domain`, `update-domain`, `approve-registration` from Cargo workspace `members` in `Cargo.toml`
- [x] 5.5 Delete source directories `domain-management/functions/register-domain/`, `domain-management/functions/update-domain/`, `domain-management/functions/approve-registration/`
- [x] 5.6 Remove `LambdaDlq` and all `DeadLetterQueue` configurations from remaining Lambda functions (DLQ behaviour now managed at the SQS queue level for command handlers; read Lambdas can retain theirs)

## 6. Verify

- [x] 6.1 Run `cargo check` -- no errors
- [x] 6.2 Run `sam validate --lint` -- valid template
