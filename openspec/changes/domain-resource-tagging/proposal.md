# Domain resource tagging

## Why

The platform provisions a full resource estate per domain (buses, schema registries, DLQs, dashboards, alarms, the namespace parameter) across the platform account and each owner account — but tags nothing. Owner-account tooling (cost allocation, governance, incident routing) sees untagged, unattributed resources; the platform's own operators lose the free filtering and console grouping tags provide. The request form meanwhile collects free-form `key=value` "custom tags" that are stored on the domain record but never applied anywhere — metadata that pretends to be tagging.

The console copy already promises "the platform applies standard tags to every domain". This change makes that true, and turns the arbitrary custom-tag field into one governed, meaningful key.

## What Changes

- **Standard tag set, always applied**: every provisioned resource carries `konductor:managed-by=konductor`, `konductor:domain=<name>`, `konductor:environment=<env>`, and `konductor:namespace=<org>-<name>`. The domain-level namespace SSM parameter carries the same minus the environment tag.
- **Governed custom tag**: the free-form tags field is replaced by a single optional `cost-center` value (request-level, validated). When present it is propagated to every provisioned resource alongside the standard set. Arbitrary tag keys are no longer accepted — a platform applies a consistent vocabulary or none.
- **Record keeps the effective set**: the domain record stores the effective tags (standard + `cost-center` when set), so describe/list render them without per-resource DescribeTags calls. The detail page shows standard and custom tags distinctly.
- **Provision-time application, immutable in v1**: tagging happens in the provisioning saga (and the trust-role path for owner-account resources). `cost-center` is set at registration and not editable in this version; re-tagging existing resources is follow-up work.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `domain-management`: registration accepts an optional governed `cost-center` instead of a free-form `tags` map; the record and read models carry the effective tag set (standard + custom).
- `domain-provisioning-workflow`: provisioning applies the standard tag set (plus `cost-center` when present) to every resource it creates, in both accounts.
- `cross-account-trust-model`: the trust role gains the tagging permissions the app-side provisioning steps need.

## Impact

- **State machines**: `provision-env` steps pass tags at resource creation (EventBridge, SQS, Schemas, CloudWatch all support create-time tags); the outer workflow tags the namespace SSM parameter.
- **Trust stack**: `konductor-trust-role` policy gains `events:TagResource` / `sqs:TagQueue` for app-side resources — existing deployments must redeploy the trust stack.
- **API**: `RegisterDomainRequest.tags` (object) is replaced by `costCenter` (string, optional). This is a breaking change to the request contract; the console is the only known caller and is updated in the same change.
- **Console**: request form swaps the key/value rows for one optional Cost center field; the domain detail page renders the effective tag set; the edit form drops tag editing.
- **Out of scope**: `data-classification` and `criticality` were considered and rejected as resource tags (event metadata is the right home for criticality); arbitrary user-defined tag keys; re-tagging on edit; tag-based IAM conditions.
