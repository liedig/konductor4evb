# Design: editable-cost-center

## Context

See proposal.md for motivation. Current state that shapes the approach:

- `update.rs` commits a single conditional `UpdateItem` (status must be `ACTIVE`) and emits `UpdateDomainSucceeded` with `domainName`, `updatedAt`, `status`, `changedFields`. `UpdateDomainBody` uses `deny_unknown_fields`, so `costCenter` is rejected at the handler today; API Gateway also rejects it because `UpdateDomainRequest` in both OpenAPI specs is `additionalProperties: false`.
- The domain record stores `costCenter` as a top-level attribute **and** embeds `cost-center` in a stored `tags` map computed by `konductor_common::tags::effective_tags` at registration. Describe renders the stored map.
- Provisioning tags these resources: per environment — platform event bus, schema registry, subscription DLQ, dashboard, two CloudWatch alarms (`{org}-{env}-{domain}-failed-invocations-alarm`, `...-dlq-depth-alarm`) in the platform account; app event bus and integration DLQ in the owner account via `{org}-konductor-trust-role` (Step Functions `Credentials` field); domain-level namespace SSM parameter in the platform account.
- The trust role already allows `events:TagResource/UntagResource` and `sqs:TagQueue/UntagQueue` on `{Organization}-*` resources (trust-stack.yaml) — exactly the two owner-account resource types, so no permission change is needed. However, the trust role's trust policy pins assumers by ARN (`KonductorRoleArn`, `ProvisioningWorkflowRoleArn`), so the trust stack gains a `RetagWorkflowRoleArn` parameter and must be redeployed in each domain account before cross-account retagging works (found during smoke testing: `States.TaskFailed` — retag role not authorized to assume the trust role).
- The domain record's `resourceArns` per-env map covers buses, registry, DLQs, and dashboard; the two alarms are not recorded but have deterministic names.
- Workflow-trigger precedent: `ProvisionDomainWorkflow` starts from an EventBridge rule on `DomainRegistrationApproved` with a DLQ (`konductor-core-stack.yaml`).
- AGENTS.md side-effects rubric: a handler performs exactly one synchronous commit; everything else is driven asynchronously. Step Functions execution names are idempotency keys, deterministic per unit of work.

## Goals / Non-Goals

**Goals:**

- `PUT /domains/{domainName}` accepts `costCenter` with registration-identical validation; committed updates rewrite `costCenter` and recompute the stored `tags` map atomically in the same `UpdateItem`.
- Committed cost-center changes converge tags on all provisioned resources without adding synchronous side effects to `update.rs`.
- Retagging is idempotent and survives event redelivery.

**Non-Goals:**

- Clearing `cost-center` (change-only semantics; the API shape makes clearing inexpressible — see Decisions).
- Retagging anything not created by the provisioning workflows (e.g., RAM shares, log groups, SNS topics).
- Backfill/audit sweeps for pre-change drift; domains untouched by this change are unaffected.

## Decisions

### 1. API accepts `costCenter` as an optional non-empty string; clearing is made inexpressible

Add `costCenter` to `UpdateDomainRequest` in `api/openapi-oauth.yaml` and `api/openapi-iam.yaml` with `minLength: 1, maxLength: 256`. Change-only semantics then fall out of the schema: an absent field means "no change" (all update fields are optional and independently applied), and an empty string is a 400 at the edge. No tri-state (`null` = clear) JSON semantics to design or document.

Alternative considered: `null` clears the tag. Rejected — it makes the async failure path carry a new validation class, requires `REMOVE costCenter` expression handling, and the user chose change-only.

### 2. Handler validates and commits; event carries `requestId` and the new `costCenter`

`UpdateDomainBody` gains `cost_center: Option<String>` (camelCase, `deny_unknown_fields` unchanged). `handle()`:

1. Validates via `tags::validate_cost_center` (control characters) — failures emit `UpdateDomainFailed` with the reason, no write.
2. Includes `costCenter` in the "no fields provided" check.
3. On commit, sets `costCenter` **and** `tags` (recomputed via `effective_tags(org, domain, None, Some(cc))`, requiring the `ORGANIZATION` env var, already used by `register.rs`) in the same `UpdateItem`, and adds `costCenter` to `changedFields`.
4. `emit_success` detail gains `requestId` (the SQS `messageId`, threaded from `handler.rs` into `update::handle` like the register path already does) and `costCenter` when changed.

The single-commit rule holds: one `UpdateItem`, one event.

Alternative considered: drive retagging off the domains-table stream. Rejected for now — the table has no stream today (the registration-stream migration is a separate recorded follow-up), and the `UpdateDomainSucceeded` event already hangs on the committed transition, which is what the retag trigger needs. A future stream migration can swap the trigger without changing the workflow contract.

### 3. New `retag-domain` STANDARD workflow triggered by an EventBridge rule

Rule on `konductor-bus`: `source = konductor.domain-management`, `detail-type = UpdateDomainSucceeded`, `detail.changedFields` contains `costCenter` (EventBridge array-contains matching), with the standard rule-failure DLQ. Target input passes the whole event.

Workflow (`domain-management/state-machines/retag-domain.asl.yaml`, YAML + JSONata):

1. **LoadDomain** — `aws-sdk:dynamodb:getItem` on the domains table for `ownerAccountId`, `environments`, `resourceArns`. Reading the committed record (not trusting event payloads beyond domainName/requestId/costCenter) keeps the tag source of truth in the table.
2. **TagNamespaceParameter** — `ssm:addTagsToResource` (upsert semantics, same as provisioning).
3. **Map over environments** (`ItemProcessor` mode INLINE), each iteration:
   - Platform account: `events:tagResource` (platform bus), `schemas:tagResource` (registry), `sqs:tagQueue` (subscription DLQ; QueueUrl derived from the ARN), `cloudwatch:tagResource` (dashboard + both alarm ARNs derived from the naming convention).
   - Owner account, via `Credentials: { RoleArn: arn:aws:iam::{ownerAccountId}:role/{org}-konductor-trust-role }`: `events:tagResource` (app bus), `sqs:tagQueue` (integration DLQ).
   - Tag value comes from the **record's** `costCenter` (post-commit), not the event.
   - Per-state Retry blocks mirroring provision-env (throttling/internal errors, exponential backoff with FULL jitter).
4. **Failure** — any exhausted state catches to a terminal `events:putEvents` emitting `RetagDomainFailed` (`domainName`, `requestId`, `failedEnvironment`, `failedStep`, `errorCode`, `errorMessage`) on the control-plane bus. No compensation and no record mutation: the record already holds the intended state, and tag operations are upserts safe to re-run, so recovery is re-driving the event/redriving the DLQ.

**Execution naming and redelivery:** executions are auto-named by the service. EventBridge rule targets cannot set a Step Functions execution name (verified against the `StartExecution` API reference, where `name` is a top-level request parameter only, and the open CDK feature request aws/aws-cdk#34731); an InputTransformer shapes only the input payload, and no reserved payload field is honored. A dispatcher Lambda was considered and rejected by decision of the maintainer: all retag API calls are idempotent upserts, so a redelivered event that starts a duplicate execution re-applies the identical tag set. The domain record remains the tag source of truth, so duplicates cannot diverge.

Alternative considered: a Rust Lambda doing all retagging with STS assume-role. Rejected — the repo's established pattern for multi-resource cross-account orchestration is ASL with `Credentials`, and the trust-role boundary stays visible in infrastructure rather than buried in code.

### 4. IAM: retag workflow gets its own role, modeled on the provisioning role

New `RetagWorkflowRole` + policy in `konductor-core-stack.yaml`: read-only DynamoDB (`GetItem` on the domains table), tag actions only (`events:TagResource`, `schemas:TagResource`, `sqs:TagQueue`, `cloudwatch:TagResource`, `ssm:AddTagsToResource`) scoped to `{Organization}-*` ARNs, `events:PutEvents` on the control-plane bus, and `sts:AssumeRole` on `arn:aws:iam::*:role/{org}-konductor-trust-role` (the same cross-account pattern the provisioning policy uses for Step Functions `Credentials`; `iam:PassRole` is not involved). Reusing `ProvisioningWorkflowRole` was considered and rejected: it carries create/delete permissions a retag path must never hold.

## Risks / Trade-offs

- [Alarm ARNs are reconstructed from the naming convention rather than read from the record] → the convention is already load-bearing (provisioning, deletion paths); a comment in the ASL marks the coupling, and a naming change would break provisioning first, where it's tested.
- [Retag is eventually consistent; cost reports can briefly mix old/new tags] → documented behavior; the record flips atomically and convergence follows in seconds. `RetagDomainFailed` plus the rule DLQ make silent divergence unlikely.
- [`UpdateDomainSucceeded` gains fields; strict downstream consumers could break] → both new fields are additive; the only in-repo consumer surface is the event contract in the spec, updated in this change.
- [Map over environments is serial INLINE] → fine at 1-3 environments; avoids EXPRESS' 5-minute ceiling and keeps per-env failure attribution. Revisit if environment counts grow.

## Migration Plan

1. Deploy core stack with the new workflow, rule, and IAM (additive; existing updates behave identically until clients send `costCenter`).
2. Redeploy the trust stack in each domain account with the new `RetagWorkflowRoleArn` parameter — without it, owner-account retagging fails with an assume-role denial and the workflow emits `RetagDomainFailed` (platform-account retagging is unaffected).
3. Deploy the web console.
4. Rollback: revert both stacks. A committed `costCenter` change made during the window keeps its record value; resources may hold the new tag without the workflow to converge further changes — harmless, and re-deploying reconverges on the next update.

## Open Questions

(none)
