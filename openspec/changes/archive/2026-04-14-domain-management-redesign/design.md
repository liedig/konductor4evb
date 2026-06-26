## Context

The domain management subsystem currently uses two DynamoDB tables (domains and registration requests), routes approval decisions through an SQS queue with a dedicated command handler Lambda, and emits duplicate events from both the Lambda handler and the Step Functions approval workflow. Status names are ambiguous and the domain lifecycle has gaps (e.g., no explicit `CREATING`/`PROVISIONING` transition). There is no notification mechanism for domain owners. The approval endpoint uses an async fire-and-forget pattern despite being a targeted human action that benefits from synchronous feedback.

This design covers the restructuring of the data model, API surface, approval flow, notification system, and Step Functions workflows to resolve these issues.

## Goals / Non-Goals

**Goals:**

- Single domain table with a clear, unambiguous status-driven lifecycle
- Synchronous decide endpoint that gives approvers immediate feedback
- Approval workflow as the sole owner of approval/rejection events and DDB status updates
- SES-based email notifications at key lifecycle transitions
- All Step Functions workflows authored in YAML with JSONata query language
- Clean separation: metadata updates vs lifecycle transitions vs deactivation (future)

**Non-Goals:**

- Domain deactivation/inactivation workflow (future, separate change)
- Admin UI for managing approvals (future; API supports it, UI is out of scope)
- Authorization model (who can approve, who can register) beyond IAM auth
- Email templating engine or notification preferences (basic SES for now)
- Migration of existing domain records (no production data exists yet)

## Decisions

### 1. Single domain table replaces domains + registration requests

Merge both tables into one. A domain record represents the entity from registration through its full lifecycle. Approval-phase fields (`taskToken`, `decidedBy`, `justification`, `approvedAt`, `rejectedAt`) live on the same record and are populated as the workflow progresses.

**Why over keeping two tables:** The registration request was always just the domain in its early lifecycle. Two tables meant redundant status tracking, cross-table consistency concerns, and a `RegistrationRequest` model that duplicated most of `Domain`. The single-table approach matches the reality: one entity, one record, status tells you where it is.

**Table schema:**

| Attribute | Type | Present |
|-----------|------|---------|
| `domainId` (PK) | String (UUID) | Always |
| `domainName` | String | Always |
| `ownerAccountId` | String (12 digits) | Always |
| `ownerEmail` | String | Always |
| `status` | String (DomainStatus) | Always |
| `description` | String | Optional |
| `tags` | Map<String, String> | Optional |
| `createdAt` | String (ISO-8601) | Always |
| `updatedAt` | String (ISO-8601) | Always |
| `taskToken` | String | During PENDING_APPROVAL only |
| `decidedBy` | String | After approval/rejection |
| `justification` | String | After approval/rejection |
| `approvedAt` | String (ISO-8601) | After approval |
| `rejectedAt` | String (ISO-8601) | After rejection |
| `resourceArns` | Map | After provisioning succeeds |
| `errorDetail` | String | On PROVISIONING_FAILED |

**GSIs (unchanged):** `by-name` (domainName), `by-owner` (ownerAccountId), `by-status` (status)

### 2. Synchronous decide endpoint replaces async SQS flow

`POST /domains/registrations/decide` is a direct API Gateway → Lambda (aws_proxy) integration. The Lambda validates the request, calls `sfn:SendTaskSuccess` or `sfn:SendTaskFailure`, and returns a synchronous response.

**Why over keeping the SQS pattern:** The approval decision is a targeted human action on a specific workflow instance identified by a task token. The caller benefits from immediate feedback ("decision recorded" vs "request not found" vs "already decided"). There is no need for durable at-least-once delivery — if the call fails, the approver retries. The SQS queue added latency, a DLQ, and a command handler Lambda that duplicated event emission.

**Request body:**

```json
{
  "taskToken": "string (required)",
  "decision": "ACCEPT | REJECT (required)",
  "decidedBy": "string (required, username)",
  "justification": "string (required on REJECT, optional on ACCEPT)"
}
```

**Responses:**

- `200 OK` — `{"message": "Decision recorded"}`
- `400 Bad Request` — validation failure (missing fields, invalid decision value)
- `500 Internal Server Error` — Step Functions callback failure (expired token, service error)

**What the Lambda does:**

1. Validate request body (taskToken present, decision is ACCEPT/REJECT, decidedBy present, justification present if REJECT)
2. If ACCEPT: call `sfn.send_task_success(taskToken, output)` where output = `{approved: true, decidedBy, justification, ...original workflow state}`
3. If REJECT: call `sfn.send_task_failure(taskToken, error: "RegistrationRejected", cause: JSON with decidedBy + justification)`
4. Return 200 on success, appropriate error on failure

**What the Lambda does NOT do:**

- Read from DynamoDB
- Emit events
- Update domain status

### 3. Approval workflow owns all approval/rejection state transitions and events

The approval workflow is the single source of truth for the approval phase. It updates the domain record status, writes `decidedBy`/`justification`/timestamps, and emits `DomainRegistrationApproved` or `DomainRegistrationRejected` events.

**Why:** Eliminates dual event emission (the old registration command handler emitted `DecideRegistrationSucceeded`/`Failed` alongside the workflow's own events). The workflow already has full context from the callback output — there is no reason for a separate handler to duplicate this work.

**Events removed:** `DecideRegistrationSucceeded`, `DecideRegistrationFailed`

### 4. Notification via Lambda + SES, triggered from the workflow

The approval workflow's `NotifyAndWait` state uses the `.waitForTaskToken` pattern to invoke a `SendApprovalNotification` Lambda. This Lambda:

1. Writes the task token to the domain record in DynamoDB (so the admin UI / API can retrieve it for platform team callers)
2. Constructs and sends an approval request email via SES to the configured platform team address
3. Returns — the workflow blocks until the callback

A separate `SendRejectionNotification` Lambda is triggered by an EventBridge rule matching `DomainRegistrationRejected`. It sends a rejection email to the domain owner's `ownerEmail` with the justification.

**Why a Lambda for email instead of direct SES SDK integration in Step Functions:** A Lambda can construct rich HTML email bodies with proper formatting, deep links, and templated content. The SFN SDK integration only supports basic string formatting via JSONata, which is limiting for email UX.

**Why the notification Lambda also persists the task token:** The task token for the `WaitForTaskToken` state only exists within that state's execution context. We cannot persist the token in a prior step and then wait in a subsequent step — they would be different tokens. The Lambda invoked by the `.waitForTaskToken` state receives the real token via `$states.context.Task.Token` and is the only place where both persistence and notification can happen with the correct token.

**SES configuration:** Parameterised in the SAM template (`ApprovalFromEmail`, `ApprovalToEmail`). SES sandbox mode for development — only verified email addresses.

### 5. Status model with explicit provisioning states

| Status | Entry point | Whose turn |
|--------|-------------|------------|
| `PENDING_APPROVAL` | Register command handler | Approver |
| `APPROVED` | Approval workflow (approved path) | System (provisioning) |
| `REJECTED` | Approval workflow (rejected path) | Terminal |
| `PROVISIONING` | Provisioning workflow (first step) | System |
| `ACTIVE` | Provisioning workflow (success) | Domain owner |
| `PROVISIONING_FAILED` | Provisioning workflow (failure) | Operator |
| `INACTIVE` | Future deactivation workflow | TBD |

**Why `PROVISIONING_FAILED` over `FAILED`:** `FAILED` is ambiguous — failed what? Registration? Approval? Provisioning? `PROVISIONING_FAILED` tells you exactly what broke and implies that operator intervention is needed on the infrastructure side.

**Why `INACTIVE` over `DEACTIVATED`:** "Inactive" is a state description ("this domain is not active"). "Deactivated" implies a specific action was taken. Since we haven't decided what triggers inactivation (admin request, policy violation, billing, expiry), the neutral term is more appropriate.

### 6. Update command scoped to metadata only

`PUT /domains/{domainId}` accepts only `description` and `tags`. The `desiredStatus` field is removed. Domain lifecycle transitions (activation, deactivation) are separate operations with their own API endpoints and workflows.

**Why:** Updating a description and deactivating a domain are fundamentally different operations with different authorization requirements, blast radius, and workflow needs. Overloading the update command with status transitions hid the severity of deactivation behind a routine metadata update.

### 7. Register command returns SQS request ID

`POST /domains` response changes from `{"message":"OK"}` to:

```json
{
  "requestId": "<SQS-MessageId>",
  "message": "Domain registration request accepted"
}
```

The `requestId` is the SQS `MessageId` returned by the `SendMessage` API call. This is available in the VTL response mapping template via the SQS response body. The domain owner can use this as a correlation handle while waiting for the email notification.

**Why SQS MessageId over generating a UUID in VTL:** The SQS response already contains a unique `MessageId`. Using it avoids the need to generate IDs in VTL and provides a direct correlation to the enqueued message for debugging.

### 8. All Step Functions use YAML + JSONata

Both the approval and provisioning workflows are rewritten in YAML with `QueryLanguage: JSONata`. ASL file extension changes from `.asl.json` to `.asl.yaml`.

**Why YAML:** More readable, especially for JSONata expressions which contain quotes and braces that conflict with JSON escaping. Multi-line strings are natural in YAML.

**Why JSONata:** More expressive than JSONPath for data transformations. Eliminates the need for `ExpressionAttributeNames` workarounds and `States.Format()` string concatenation chains. The provisioning workflow's CloudWatch dashboard definition, currently an unreadable single-line `States.Format` call with 7+ positional arguments, becomes a readable JSONata expression.

### 9. taskToken handling

The task token is an ephemeral, single-use credential generated by Step Functions. It is:

- Written to the domain record by the `SendApprovalNotification` Lambda during the `NotifyAndWait` step
- Included in the approval email sent to the platform team
- Submitted by the approver via `POST /domains/registrations/decide`
- Consumed by `sfn:SendTaskSuccess`/`sfn:SendTaskFailure` and invalidated
- Never returned in `GET /domains` or `GET /domains/{domainId}` responses to domain owners
- Accessible to platform team callers via the describe endpoint when status is `PENDING_APPROVAL` (authorization mechanism TBD)

## Risks / Trade-offs

**Task token as the only approval handle** — If the email is lost or the token expires (7-day timeout), the approval workflow times out and the domain is stuck in `PENDING_APPROVAL`. Mitigation: the admin UI (future) can retrieve the token via the API, and a re-notification mechanism can be added later. For now, the platform team can query DynamoDB directly as a fallback.

**SES sandbox limitations** — In sandbox mode, both sender and recipient must be verified email addresses. This is fine for development but requires SES production access request before real use. Mitigation: document the SES setup steps, parameterise all email addresses.

**Single table means approval-phase fields on every domain record** — Fields like `taskToken`, `decidedBy`, `justification` are only relevant during the approval phase but live on the domain record permanently. Trade-off accepted: the simplicity of one table outweighs the cost of a few nullable fields. These fields serve as an audit trail after approval.

**Synchronous decide endpoint has no retry/DLQ safety net** — If the `SendTaskSuccess` call fails, the approver gets an error and must retry manually. Unlike the SQS pattern, there is no automatic retry. Trade-off accepted: the approver is a human who can retry, and the failure modes (expired token, network error) are better communicated synchronously than buried in a DLQ.

**No migration path defined** — This design assumes no production data exists. If domains have been registered in the current two-table model, a migration would be needed. Current assessment: no production data, so this is not a risk today.

## Open Questions

- **Authorization for taskToken access:** How do we distinguish platform team callers from domain owner callers on the describe endpoint? IAM policy conditions? Separate endpoint? Deferred to a future authorization change.
- **Notification on provisioning completion:** Should the domain owner receive an email when provisioning succeeds (domain is ACTIVE)? The proposal mentions it as a possibility. If yes, this would be a third notification Lambda triggered by `DomainCreationSucceeded`.
- **`GET /domains/registrations` endpoint:** Keep as a convenience alias for `GET /domains?status=PENDING_APPROVAL`, or remove in favour of the filtered list? The admin UI might benefit from a dedicated registrations view.
- **Retry path for `PROVISIONING_FAILED`:** Is re-registration the only recovery, or should there be a "retry provisioning" command? Deferred to a future change.
