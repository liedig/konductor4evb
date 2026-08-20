# Design: domain resource tagging

## Context

Provisioning is a Step Functions saga (`provision-domain` → per-env `provision-env`) creating resources via direct SDK integrations in the platform account and via the assumed trust role in the owner account. None of those calls carry tags today. The domain record stores a free-form `tags` map from the request, applied nowhere. AWS allows 50 user tags per resource on every service we use; `aws:`-prefixed system tags (e.g. CloudFormation's) don't count and don't appear anyway, since the saga creates resources via SDK, not CFN.

## Goals / Non-Goals

**Goals:**
- Every provisioned resource is attributable at a glance: platform-managed, which domain, which environment, which namespace
- One governed custom dimension (`cost-center`) for owner-account cost allocation
- A consistent vocabulary: no arbitrary tag keys
- The console renders the effective tag set without per-resource API calls

**Non-Goals:**
- `data-classification` and `criticality` as resource tags (rejected in review; criticality is reserved as a future *event metadata* dimension)
- Arbitrary user-defined tag keys
- Re-tagging existing resources on record edits (provision-time only in v1)
- Tag-based IAM authorization conditions
- Backfill tagging of already-provisioned domains (the demo estate is rebuilt per test cycle; a backfill task can follow if a real estate accumulates)

## Decisions

1. **Tag keys use the `konductor:` namespace.** Collision-proof against any owner-account tag taxonomy and self-describing in the console. Values are derived, never free-typed: `managed-by` is a constant, `domain`/`environment`/`namespace` resolve from the record + `NamespaceResolver`.

2. **`costCenter` replaces `tags` in the registration contract.** A single optional string field, validated for AWS tag-value legality (≤256 chars, no control characters). The free-form `tags` map disappears from the API, the record, and the console. Breaking change, accepted: the console is the only caller and ships in the same change.

3. **Tags ride the create calls, not a separate tagging pass.** EventBridge `CreateEventBus`, Schemas `CreateRegistry`, SQS `CreateQueue`, CloudWatch `PutDashboard`/`PutMetricAlarm`, and SSM `PutParameter` all accept tags at creation. The saga steps gain a Tags/TagSpecifications argument computed from a single shared helper (`effectiveTags(record, env)`) so no step can drift from the model. Where a service needs a post-create tag call instead, the call is an explicit saga step so failures compensate like any other step.

4. **The record is the rendering source of truth.** Registration computes the effective set (standard + `cost-center`) onto the record at write time; describe/list render from the record. This keeps reads free of N resource-service calls and survives resource re-creation.

5. **`cost-center` is immutable in v1.** The update path rejects it; the edit form drops tag editing. Re-tagging on edit spans both accounts and the trust role — real work, deferred deliberately rather than half-done.

6. **Trust role gains scoped tag permissions** (`events:TagResource`/`UntagResource`, `sqs:TagQueue`/`UntagQueue` on `{org}-*`). This is a trust-stack redeploy for existing installations — called out in the operator docs and the change verification.

## Risks / Trade-offs

- **Contract break**: any external script posting `tags` breaks with 400. Accepted: pre-1.0, single known caller.
- **Immutability surprise**: an owner who mistypes a cost center must re-register (or wait for the follow-up). Mitigated by console validation and the confirmation copy.
- **Saga step growth**: tagging args add payload but no new states except where a service demands a post-create call; compensation unchanged (deletion takes the tags with the resource).
