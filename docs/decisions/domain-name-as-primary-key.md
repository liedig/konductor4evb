# Replace UUID domainId with domainName as DynamoDB partition key and sole domain identifier

[Documentation home](../README.md)

- Recorded date: 2026-06-26
- Change ID: `domain-name-as-primary-key`
- Area: Domain Management
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Replaced UUID (`domainId`) with `domainName` as the DynamoDB partition key, aligning with DDD/EDA principles where the domain name IS the bounded context identifier. Global name uniqueness is an intentional governance feature forcing explicit bounded-context decisions. Removed the `by-name` GSI (redundant with partition key). Uniqueness now enforced atomically via conditional PutItem with `attribute_not_exists(domainName)`, eliminating the pre-write GSI query and its race condition. API paths changed from `/domains/{domainId}` to `/domains/{domainName}`. Event payloads dropped `domainId` from all 8 event types. Documented account-per-domain as an explicit design principle: one AWS account hosts one domain's resources, with primary/secondary owner model where secondary owners are co-managers acting as backup decision-makers. Breaking change for any event consumers keying off `domainId`.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-06-26-domain-name-as-primary-key/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
