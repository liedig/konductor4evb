## Why

The domain name is the natural identity of a bounded context in DDD/EDA terms, yet the system uses an opaque UUID (`domainId`) as the DynamoDB partition key. This creates friction: after `POST /domains` (async via SQS), the consumer receives an SQS message ID, not the domain identifier, and must poll to discover the UUID. A `by-name` GSI exists solely as a workaround for looking up domains by their actual identity. Domain names are effectively immutable (not updatable, and renaming would require recreating all infrastructure resources), so the UUID provides no renamability benefit. Global name uniqueness is an intentional governance feature that forces explicit bounded-context decisions, not an accident to work around.

## What Changes

- **BREAKING**: DynamoDB partition key changes from `domainId` (UUID v4) to `domainName` (string). The `by-name` GSI is removed (redundant with the partition key). The `by-owner` and `by-status` GSIs are retained.
- **BREAKING**: API paths change from `/domains/{domainId}` to `/domains/{domainName}` for the `GET` (describe) and `PUT` (update) endpoints.
- **BREAKING**: Event payloads drop `domainId` from all event types. `domainName` becomes the sole domain identifier in `RegisterDomainSucceeded`, `RegisterDomainFailed`, `UpdateDomainSucceeded`, `UpdateDomainFailed`, `DomainRegistrationApproved`, `DomainRegistrationRejected`, `DomainProvisioningSucceeded`, and `DomainProvisioningFailed`.
- **BREAKING**: Step Functions workflow inputs (approval, provision-domain, provision-env, deprovision-env) replace `domainId` with `domainName` as the primary domain identifier.
- The register handler stops generating a UUID and uses the provided `domainName` as the DynamoDB partition key. Uniqueness is enforced by DynamoDB's primary key constraint (no need for a pre-write GSI query).
- The OpenAPI spec path parameter changes from `domainId` to `domainName`.
- The API Gateway VTL mapping template for `PUT /domains/{domainName}` updates the path parameter injection.
- The smoke test simplifies: no DynamoDB polling to discover the domain identifier.
- Document the account-per-domain design principle and the primary/secondary owner model as explicit design decisions (secondary owners are co-managers of the same account, acting as backup decision-makers; resources always live in the primary owner's account).

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `domain-management`: Domain persistence requirement changes (partition key = `domainName`, remove `by-name` GSI). Event payloads drop `domainId`. Register, describe, and update requirements change to reference `domainName` as the identifier. List domains response uses `domainName` as the primary field.
- `domain-provisioning-workflow`: Workflow inputs and event payloads replace `domainId` with `domainName`. The `provision-env` inner workflow accepts `domainName` instead of `domainId`.
- `async-command-processing`: API path patterns change from `/domains/{domainId}` to `/domains/{domainName}`. VTL template path parameter injection updates.
- `system-architecture`: API path patterns change from `/domains/{domainId}` to `/domains/{domainName}`.
- `installation`: Smoke test and manual examples updated to use `domainName` in API paths. The post-register polling step is simplified.

## Impact

- **DynamoDB**: Table schema change (partition key `domainId` → `domainName`, remove `by-name` GSI). Requires table recreation — acceptable since Konductor is pre-production.
- **Lambda functions**: `register.rs` (stop UUID generation, use domainName as key), `describe-domain` (query by domainName), `update.rs` (query by domainName), `list-domains` (response shape), `decide-registration` (no domainId in workflow callbacks).
- **OpenAPI spec**: Path parameter rename on `/domains/{domainName}` endpoints.
- **Step Functions**: All four state machine definitions (approval, provision-domain, provision-env, deprovision-env) update workflow inputs to use `domainName`.
- **Event payloads**: All eight event types drop `domainId`. Breaking for any downstream event consumers.
- **Makefile**: Smoke test target simplifies (no DynamoDB polling for domain ID discovery).
- **Documentation**: `docs/installation.md` manual smoke test examples, `README.md` API section.
- **No new infrastructure**: No new AWS resources are created or removed (beyond the DynamoDB table recreation).
