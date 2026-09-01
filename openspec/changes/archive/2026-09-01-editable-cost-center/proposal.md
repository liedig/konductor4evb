# Proposal: editable-cost-center

## Why

`cost-center` is the one governed custom tag, used for cost allocation, but it can only be set at registration. When a team's cost code changes (reorg, finance re-mapping), the domain record and every provisioned resource keep the stale value forever; the console even documents "cost center is not editable". Owners need to correct it without decommissioning and re-registering the domain.

## What Changes

- The update-domain command (`PUT /domains/{domainName}`) accepts `costCenter` in addition to description and owner emails. The same registration-time validation applies (non-empty, ≤256 chars, no control characters).
- Change-only semantics: an update MAY set `costCenter` where none exists and MAY replace an existing value with another valid value; it SHALL NOT clear an existing value (empty/absent removal is rejected).
- Updating `costCenter` rewrites the stored `costCenter` attribute and recomputes the stored effective `tags` map on the domain record, so describe/list render the new value immediately.
- A committed `costCenter` change triggers a new `retag-domain` Step Functions workflow (via an EventBridge rule on `UpdateDomainSucceeded` with `costCenter` in `changedFields`) that re-tags every provisioned resource recorded in the domain's `resourceArns` map — per-environment event buses, schema registries, DLQs, dashboards and alarms in both platform and owner accounts (owner account via `konductor-trust-role`), plus the domain-level namespace SSM parameter.
- `UpdateDomainSucceeded` detail gains `requestId` (the SQS message id of the update command) and, when changed, `costCenter`. The retag workflow is started directly by the rule with an auto-generated execution name (rule targets cannot set execution names); tag operations are idempotent upserts, so a redelivered event that starts a duplicate execution converges to the same tag state.
- The web console edit form gains a cost-center input; the "cost center is not editable" hint is removed.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `domain-management`: the Update Domain requirement gains `costCenter` as a mutable field with change-only semantics; the Governed domain tag model requirement drops "SHALL NOT be editable afterwards" and defines validation, record rewrite, and event enrichment; the "Cost center is immutable" scenario is replaced.
- `domain-provisioning-workflow`: adds a retag-domain workflow that converges provisioned-resource tags after a committed `costCenter` change, including cross-account retagging via the trust role.
- `web-ui`: the domain edit form exposes cost center as an editable field.

## Impact

- **API contracts**: `UpdateDomainRequest` gains an optional `costCenter` (maxLength 256) in `api/openapi-oauth.yaml` and `api/openapi-iam.yaml`; `UpdateDomainSucceeded` detail gains `requestId` and `costCenter`.
- **Backend**: `domain-registrations/src/update.rs` (new field, validation, tags-map recompute, enriched event); `handler.rs` threads the SQS message id into the update path.
- **Workflows/infra**: new `domain-management/state-machines/retag-domain.asl.yaml`; `konductor-core-stack.yaml` gains the state machine, its EventBridge rule (with DLQ), IAM permissions (DynamoDB read, cross-service tag actions, `iam:PassRole` for the trust role); `trust-stack.yaml` gains tag permissions in owner accounts (EventBridge, SQS, Schemas, CloudWatch, SSM as applicable).
- **Web**: `web/app/pages/domains/[name].vue` edit modal, `web/app/types/api.ts` `UpdateDomainRequest`.
- **Docs**: `docs/development/decisions.md` (side-effects classification for the retag path), `docs/domain-management.md` examples.
