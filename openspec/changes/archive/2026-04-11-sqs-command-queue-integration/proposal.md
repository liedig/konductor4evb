## Why

All control plane write operations (register, update, decide registration) are synchronously invoked by API Gateway, meaning a Lambda failure mid-execution silently loses the command. Routing write operations through SQS provides durable command delivery with automatic retries and dead-letter capture, ensuring no accepted command is ever lost. This also decouples API response latency from command processing time.

## What Changes

- **BREAKING**: All write API operations (POST, PUT) return `202 Accepted` with `{"message":"OK"}` instead of synchronous results. Callers must rely on EventBridge events for outcomes.
- Replace `aws_proxy` Lambda integrations with `aws` service integrations to SQS for all write paths (POST/PUT on `/domains`, POST on `/domains/registrations/{requestId}/decision`)
- Add API Gateway request body validation (`x-amazon-apigateway-request-validator`) using JSON schema models for structural validation on all write operations
- Introduce two SQS command queues with dedicated DLQs:
  - `domains-command-queue` for POST/PUT on `/domains` and `/domains/{domainId}`
  - `registrations-command-queue` for POST on `/domains/registrations/{requestId}/decision`
- Replace per-operation write Lambda functions with two command handler Lambdas (one per queue) that dispatch on the `HttpMethod` message attribute
- Add VTL request templates that inject path parameters and HTTP method as SQS message attributes, following the [Unicorn Contracts pattern](https://github.com/aws-samples/aws-serverless-developer-experience-workshop-python)
- Each command handler publishes outcome events to the control plane EventBridge bus (e.g., `RegisterDomainSucceeded`, `RegisterDomainFailed`, `UpdateDomainSucceeded`, `UpdateDomainFailed`, `DecideRegistrationSucceeded`, `DecideRegistrationFailed`)
- Remove the existing shared `LambdaDlq` and per-function `DeadLetterQueue` configuration for write functions (replaced by per-queue DLQs)
- Read operations (GET) remain unchanged as synchronous `aws_proxy` Lambda integrations

## Capabilities

### New Capabilities

- `async-command-processing`: SQS-backed durable command queue pattern for all control plane write operations, including queue topology, VTL integration templates, command handler dispatch, and outcome event publishing

### Modified Capabilities

- `domain-management`: Write operations become asynchronous; register, update, and decide-registration no longer return synchronous results. New outcome event types added (`RegisterDomainSucceeded/Failed`, `UpdateDomainSucceeded/Failed`, `DecideRegistrationSucceeded/Failed`)
- `system-architecture`: API layer changes from one-Lambda-per-write-operation with `aws_proxy` to SQS service integrations with consolidated command handler Lambdas. Read operations remain one-Lambda-per-operation

## Impact

- **API contract**: All write responses change from operation-specific payloads to `202 {"message":"OK"}`. Breaking change for all API consumers.
- **OpenAPI spec** (`api/openapi.yaml`): Write path integrations rewritten from `aws_proxy` to `aws` type with VTL templates. Request validator added.
- **SAM template** (`template.yaml`): New SQS queues, DLQs, IAM roles for API GW -> SQS. Write Lambda functions consolidated. Read Lambda functions unchanged.
- **Lambda functions**: `register-domain`, `update-domain`, `approve-registration` functions replaced by `domain-command-handler` and `registration-command-handler`. Handler input changes from API Gateway proxy event to SQS event.
- **Shared library** (`lib/control-plane-common`): SQS event parsing utilities needed. API response helpers for write paths no longer used by command handlers (kept for read paths).
- **EventBridge**: New outcome event detail-types added to control plane bus.
