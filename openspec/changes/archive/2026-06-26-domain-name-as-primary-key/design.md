## Context

Nexus uses a UUID v4 (`domainId`) as the DynamoDB partition key for the `nexus-domains` table. The domain name is enforced globally unique via a pre-write query against the `by-name` GSI. API paths use the UUID (`/domains/{domainId}`), and all event payloads include both `domainId` and `domainName`. The register endpoint is asynchronous (API Gateway → SQS → Lambda), so consumers receive an SQS message ID rather than the domain identifier, forcing them to poll by name to discover the UUID.

The system is pre-production with a single smoke-test domain deployed across two AWS accounts. The account-per-domain strategy is the current design principle: one AWS account hosts one domain's application-side resources, with a trust stack per account. Primary and secondary owners are co-managers of the same account; the secondary acts as backup decision-maker. Resources always live in the primary owner's account.

## Goals / Non-Goals

**Goals:**
- Replace `domainId` (UUID) with `domainName` as the DynamoDB partition key
- Remove the `by-name` GSI (redundant with the partition key)
- Use DynamoDB conditional writes for atomic uniqueness enforcement (replace the pre-write GSI query)
- Update API paths, event payloads, and Step Functions inputs to use `domainName` as the sole domain identifier
- Simplify the consumer experience: after `POST /domains`, the consumer already knows the identifier they provided
- Document the account-per-domain principle and primary/secondary owner model as explicit design decisions

**Non-Goals:**
- Implementing the primary/secondary owner model (data model change only; ownership logic is future work)
- Multi-region support (parked; would require a composite key `domainName + region`)
- Domain name renamability (domain names remain immutable; renaming requires delete + re-register)
- Changing the async command pattern (API Gateway → SQS → Lambda remains)
- Changing the approval workflow's task-token mechanism

## Decisions

### 1. `domainName` as DynamoDB partition key with conditional PutItem

The register handler currently generates a UUID, queries the `by-name` GSI to check for duplicates, then writes with the UUID as the partition key. This is a two-step check-then-write that is susceptible to race conditions under concurrent registrations.

With `domainName` as the partition key, the handler uses a conditional `PutItem` with `attribute_not_exists(domainName)`. DynamoDB rejects the write atomically if the name already exists, eliminating both the GSI query and the race condition.

**Alternative considered:** Keep the UUID as PK but expose `domainName` in API paths with an internal lookup. Rejected because it adds a lookup step without simplifying anything, and the UUID provides no benefit (domain names are immutable).

### 2. Remove the `by-name` GSI

The `by-name` GSI exists solely to look up domains by name when the UUID is the partition key. With `domainName` as the partition key, direct `GetItem` by name replaces GSI queries. The `by-owner` and `by-status` GSIs are retained (they serve different query patterns not covered by the partition key).

### 3. API paths use `domainName`

`GET /domains/{domainName}` and `PUT /domains/{domainName}` replace `GET /domains/{domainId}` and `PUT /domains/{domainId}`. The OpenAPI spec path parameter and the API Gateway VTL mapping template for the PUT endpoint update accordingly. The `POST /domains` endpoint is unchanged (the domain name is in the request body, not the path).

Domain names are URL-safe (lowercase letters, digits, and hyphens only, enforced by the existing validation regex `^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$`), so they work directly in URL paths without encoding.

### 4. Event payloads drop `domainId`

All eight event types currently include both `domainId` and `domainName` in their payloads. Since `domainName` is now the sole identifier, `domainId` is removed from all payloads. The `domainName` field was already present in every event, so consumers that key off `domainName` are unaffected. Consumers that key off `domainId` must update.

### 5. Step Functions inputs use `domainName`

The approval workflow's `StartExecution` input, the provisioning workflow's event-trigger input, and the inner `provision-env` / `deprovision-env` workflow inputs all currently pass `domainId`. These change to pass `domainName` as the domain identifier. The approval workflow's task-token callback output also drops `domainId` (it was not used by the callback consumer anyway — the task token is the correlation mechanism).

### 6. Account-per-domain as a documented design principle

The system assumes one AWS account per domain for application-side resources. This is documented as an explicit design decision, not an accident. The trust stack is deployed per-account, and resource naming (`{org}-{env}-{domain}-*`) assumes no two domains share an account. This principle should be captured in the system-architecture spec.

### 7. Primary/secondary owner model documented as a design constraint

The current `ownerAccountId` field represents the primary owner. The data model will eventually evolve to support secondary owners (co-managers of the same account, acting as backup decision-makers). This refactor does not implement that model but documents it as a design constraint: the `ownerAccountId` field semantics are "the account where this domain's resources live," and secondary ownership is a future authorization concern, not a data model concern. The partition key choice (`domainName`) is compatible with this future evolution.

## Risks / Trade-offs

- **[Risk] DynamoDB table recreation required** -> Changing the partition key is not an in-place update; CloudFormation replaces the table. Since the table has `DeletionPolicy: Delete` and the system is pre-production with one smoke-test domain, this is a clean cutover. The core stack redeploy handles it automatically.

- **[Risk] Breaking change for event consumers** -> Any system subscribing to Nexus events that keys off `domainId` will break. Since Nexus is pre-production with no external consumers, this is acceptable. The `RegisterDomainFailed` event already does not include `domainId` (it only has `domainName`), so the pattern of using `domainName` as the identifier already exists partially.

- **[Risk] Conditional PutItem error handling** -> When a duplicate domain name is submitted, DynamoDB returns a `ConditionalCheckFailedException`. The register handler must catch this and emit `RegisterDomainFailed` with a clear "domain already exists" message, replacing the current pre-write GSI query + manual check. The error handling path changes but the observable behavior (failed event with reason) remains the same.

- **[Trade-off] No re-registration of deleted domain names** -> With `domainName` as the partition key, a deleted (INACTIVE) domain's DynamoDB item persists with that name, blocking re-registration. This is the same behavior as today (the GSI query does not filter by status). If re-registration becomes a requirement, the solution is to hard-delete the DynamoDB item on domain deletion rather than marking it INACTIVE. This is a future concern, not a regression.

- **[Trade-off] Global uniqueness enforced by infrastructure** -> DynamoDB's partition key uniqueness is global within the table. This aligns with the DDD governance principle (forces explicit bounded-context decisions) but means two teams cannot register domains with the same name, even in different accounts. This is intentional.
