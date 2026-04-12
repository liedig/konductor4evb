## Context

All write operations currently use `aws_proxy` API Gateway integrations that invoke Lambda functions synchronously. A Lambda crash or transient AWS SDK failure mid-execution silently loses the command. This change routes all writes through SQS so every accepted command is durably held until successfully processed, with automatic retries and DLQ capture on exhausted retries.

The design follows the pattern established by the [AWS Unicorn Contracts sample](https://github.com/aws-samples/aws-serverless-developer-experience-workshop-python): API Gateway sends directly to SQS using a simple VTL mapping template that passes the raw request body as the message body and the HTTP method as a message attribute. A single Lambda per queue dispatches on the `HttpMethod` attribute.

## Goals / Non-Goals

**Goals:**
- Route all write operations (POST/PUT on `/domains`, POST on `/domains/registrations/{requestId}/decision`) through SQS
- API Gateway validates request body structure before enqueueing
- All writes return `202 Accepted` with `{"message":"OK"}`
- Two command queues: `domains-command-queue` and `registrations-command-queue`, each with a dedicated DLQ
- Two command handler Lambdas replace the three existing write Lambdas (`register-domain`, `update-domain`, `approve-registration`)
- Each handler emits `*Succeeded` or `*Failed` outcome events to the control plane EventBridge bus
- Read operations (GET) remain synchronous and unchanged

**Non-Goals:**
- Command status polling API (callers consume EventBridge events instead)
- Per-caller event delivery (EventBridge subscriptions are the consumer's responsibility)
- Changing the approval workflow or provisioning workflow internals

## Decisions

### One queue per resource group, not per operation

Two queues are used: one for domain commands (POST/PUT on `/domains` and `/domains/{id}`), one for registration commands (POST on `/domains/registrations/{requestId}/decision`). This is a middle ground between one-queue-per-operation (more isolation, more infrastructure) and a single queue (simpler, but couples all operations).

The domains queue handles both register and update because they share the same resource path family and similar processing weight. The registrations queue is isolated because `DecideRegistration` is time-sensitive (Step Functions task tokens expire) and should not be delayed by a burst of domain registrations.

**Alternative considered**: Single queue for everything. Rejected because task token expiry makes `DecideRegistration` sensitive to queue depth driven by unrelated operations.

### HTTP method as the sole routing key

Within each queue, dispatch uses only the `HttpMethod` message attribute (`POST` vs `PUT`). No separate `operation` attribute is needed because:
- The domains queue: `POST` = register, `PUT` = update
- The registrations queue: only `POST` is valid

This is the simplest routing possible and matches the reference pattern exactly.

**Alternative considered**: Injecting an explicit `Operation` attribute in VTL. Rejected as unnecessary complexity given the one-to-one mapping of method to operation within each queue.

### Path parameters injected into message body via VTL

API Gateway VTL templates wrap the original request body to include path parameters:

```vtl
#set($body = $input.json('$'))
{
  "domainId": "$input.params('domainId')",
  "body": $body
}
```

For the registrations queue:
```vtl
{
  "requestId": "$input.params('requestId')",
  "body": $body
}
```

For the `/domains` POST (no path params), the message body is `$input.body` directly, matching the Unicorn pattern verbatim.

**Alternative considered**: Requiring clients to include IDs in the request body. Rejected because it duplicates path parameters and creates redundancy the handler would need to reconcile.

### Existing write Lambda functions replaced by two command handlers

`register-domain`, `update-domain`, and `approve-registration` are removed. Two new Lambdas replace them:
- `domain-command-handler` -- consumes `domains-command-queue`
- `registration-command-handler` -- consumes `registrations-command-queue`

Shared business logic (DynamoDB access, event emission, error types) stays in `lib/control-plane-common`.

**Alternative considered**: Keep existing Lambdas, add SQS triggers to them. Rejected because the event shape changes entirely (SQS event vs API GW proxy event) and the existing handlers are tightly coupled to the proxy event structure.

### IAM role for API Gateway -> SQS

A dedicated IAM role with `sqs:SendMessage` permissions scoped to the two command queues is created for the API Gateway integration. This follows least-privilege and is separate from any Lambda execution role.

### Outcome events replace synchronous responses

Callers can no longer receive synchronous operation results. Instead, they subscribe to EventBridge events: `RegisterDomainSucceeded/Failed`, `UpdateDomainSucceeded/Failed`, `DecideRegistrationSucceeded/Failed`. This is consistent with the project's event-driven philosophy and allows any downstream system (email, dashboard, webhook) to react without coupling to the API layer.

### Shared LambdaDlq removed for write functions

The existing `LambdaDlq` SQS queue was used as a `DeadLetterQueue` on the write Lambda functions for async invocation failures. With SQS ESM, retry and DLQ behaviour is managed at the queue level. The per-function `DeadLetterQueue` configuration is removed from write Lambda resources. Read Lambdas retain their DLQ configuration since they remain directly invoked.

## Risks / Trade-offs

**[Risk] All writes become fire-and-forget from the caller's perspective** -> Callers must subscribe to EventBridge to learn outcomes. This is a breaking change and requires consumer adaptation. Mitigation: clear documentation, API versioning if needed, and ensuring outcome events have rich enough payloads.

**[Risk] Task token expiry for DecideRegistration** -> If the registrations queue backs up, task tokens may expire before the message is processed. Mitigation: dedicated queue with short visibility timeout (30s), alarm on `ApproximateAgeOfOldestMessage`, and a generous task token timeout in the approval workflow ASL.

**[Risk] Structural validation at API Gateway cannot enforce business rules** -> Duplicate domain names or invalid state transitions are only caught in the handler, after enqueueing. Callers receive `202` then a `*Failed` event. Mitigation: this is acceptable for a control plane; the latency between acceptance and failure notification is low (seconds).

## Migration Plan

1. Add `domains-command-queue` + DLQ and `registrations-command-queue` + DLQ to `template.yaml`
2. Add IAM role for API Gateway -> SQS (`ControlPlaneApiSqsRole`)
3. Replace `aws_proxy` write integrations in `api/openapi.yaml` with `aws` SQS integrations and VTL templates; add `x-amazon-apigateway-request-validator`
4. Create `domain-management/functions/domain-command-handler` and `registration-command-handler`
5. Remove `register-domain`, `update-domain`, `approve-registration` from `template.yaml` and Cargo workspace
6. Update write operation API responses from `201/200` to `202` in OpenAPI
7. Run `cargo check` and `sam validate`

**Rollback**: Revert to prior SAM stack version. No data migration needed.
