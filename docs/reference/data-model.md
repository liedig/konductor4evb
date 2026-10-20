# Data model

[Documentation home](../README.md)

Domain and subscription storage keys and access patterns. Binding and entitlement storage live with their [authorization explanation](../architecture/authorization.md).

> **Review pending:** [D10](../reference/documentation-discrepancies.md#d10) records conflicting or unverified guidance retained from the previous documentation. This reorganization does not resolve it.

## Domains table

A single `domains` DynamoDB table stores both domain metadata and registration state. This was a deliberate choice over separate tables—the domain entity and its registration lifecycle are the same concern.

### Table schema

| Attribute | Type | Key |
|-----------|------|-----|
| `domainName` | String | Partition key (PK) |
| `ownerAccountId` | String | GSI: `by-owner` (PK) |
| `status` | String | GSI: `by-status` (PK) |
| `description` | String | Not applicable |
| `ownerEmail` | String | Not applicable |
| `secondaryOwnerEmails` | String Set (SS) | Not applicable |
| `tags` | Map | Not applicable |
| `environments` | List<String> |—(stamped at registration time from the platform `Environments` parameter) |
| `environmentAccounts` | Map<String,String> |—(sparse per-environment account overrides, stored exactly as submitted; keys are members of the stamped environment list, values are 12-digit account IDs; absent when the registration carried no overrides and on records predating the field) |
| `resourceArns` | Map<env, PerEnvResourceArns> |—(populated after provisioning; keyed by environment) |
| `namespaceParameter` | String |—(for example, `/namespace/billing`) |
| `taskToken` | String |—(Step Functions task token) |
| `decidedBy` | String | Not applicable |
| `justification` | String | Not applicable |
| `createdAt` | String (ISO 8601) | Not applicable |
| `updatedAt` | String (ISO 8601) | Not applicable |
| `approvedAt` | String (ISO 8601) | Not applicable |
| `rejectedAt` | String (ISO 8601) | Not applicable |
| `errorDetail` | String | Not applicable |

### Access patterns

| Pattern | Index | Key condition |
|---------|-------|---------------|
| Get domain by name | Table (PK) | `domainName = :name` |
| List domains by owner | `by-owner` | `ownerAccountId = :acct` |
| List domains by status | `by-status` | `status = :status` |
| Detect new account | `by-owner` | `ownerAccountId = :acct` + filter `status = ACTIVE` |

The table uses `PAY_PER_REQUEST` billing with point-in-time recovery enabled and a `Retain` deletion policy.

### Domain-count ceiling

The table is capped at **1,000 rows**. A singleton counter item (`domainName = "META#domain-count"`, attribute `count`) tracks the total; every row-creating registration write (new `PENDING_APPROVAL` registrations and collision-preflight `REJECTED` rows) runs as a DynamoDB transaction that creates the row and increments the counter atomically, conditioned on the counter staying below the cap. Re-registration of a rejected domain overwrites its existing row in a follow-up transaction without charging the counter.

The counter item is not a domain: it carries no `ownerAccountId` or `status`, so it never enters the `by-owner`/`by-status` GSIs, and both list-domains scans exclude it by key. At the cap, registration fails deterministically with `RegisterDomainFailed` (domain-limit reason), no approval workflow starts, and the message is acknowledged without retry. Rows are never deleted, so the ceiling bounds lifetime registrations until a domain-deletion capability lands; `docs/service-quotas.md` holds the seeding/reconciliation runbook and the hourly `domain-count-reporter` Lambda feeds the 80%-of-cap alarm (`Konductor/DomainManagement` `DomainCount`).

### Event streams table

The `konductor-event-streams` table is the **source of intent** for subscriptions; EventBridge rules are the materialization. Topology questions are answered from the table, never by enumerating rules across buses.

| Attribute | Type | Key |
|-----------|------|-----|
| `producerDomain` | String | Partition key (PK) |
| `schemaConsumer` | String (`{schemaName}#{consumerDomain}`) | Sort key (SK) |
| `consumerDomain` | String | GSI: `by-consumer` (PK) |
| `source` / `detailType` / `schemaName` | String | Not applicable |
| `status` | String (`PENDING\|ACTIVE\|FAILED\|REMOVED`) | Not applicable |
| `environments` | Map<env, {pinnedVersion, ruleName, ruleArn, enabled, status}> | Not applicable |
| `createdAt` / `updatedAt` / `removedAt` | String (ISO 8601) | Not applicable |
| `expiresAt` | Number (epoch) | TTL (removal + 90 days) |

| Pattern | Index | Key condition |
|---------|-------|---------------|
| Subscription by identity | Table (PK+SK) | `producerDomain = :p AND schemaConsumer = :schema#consumer` |
| Version census for a schema | Table (PK+SK) | `producerDomain = :p AND begins_with(schemaConsumer, :schema#)` |
| Streams feeding a consumer | `by-consumer` | `consumerDomain = :c` |

The API-facing subscription id is `{consumerDomain}.{producerDomain}.{detailType}`—domain names cannot contain dots, so parsing splits on the first two dots; event names may contain dots safely.

### Schema promotions table

The `konductor-schema-promotions` table holds promotion head items and re-pin coordination state. The head item doubles as the per-target lease (a conditional put rejects a concurrent promotion with `409`), and its `NEW_AND_OLD_IMAGES` stream starts the promotion workflow and emits the lifecycle events.

| Attribute | Type | Key |
|-----------|------|-----|
| `pk` | String (`{domain}#{eventName}#{environment}`) | Partition key (PK) |
| `sk` | String (`PROMOTION` or `REPIN`) | Sort key (SK) |

`PROMOTION` items carry `promotionId`, `sourceEnvironment`, `targetEnvironment`, `fromVersion`, `requestedVersion`, `lastWrittenVersion`, `phase` (`VALIDATING\|REPLAYING\|REPINNING\|COMPLETED\|FAILED\|CANCELLED`), `reason` (`DIVERGED\|REPLAY_FAILED\|SKIPPED_ENVIRONMENT`), `failedVersion`, `actor`, the `repinMatched`/`repinRepinned`/`repinFailed` counts, `drainUntil`, `createdAt`/`updatedAt`, and `expiresAt` (TTL). `REPIN` items carry `floorVersion`, `targetVersion`, `generation` (the optimistic-concurrency token that lets a newer re-pin win narrowing), `drainUntil`, and `updatedAt`; outside a re-pin, `floorVersion == targetVersion` is the pinned version and the derived rule pattern is `[pinned, pinned + 1]`.

| Pattern | Index | Key condition |
|---------|-------|---------------|
| Promotion status for a schema and target | Table (PK+SK) | `pk = :d#e#env AND sk = PROMOTION` (strongly consistent) |
| Re-pin state for a schema and environment | Table (PK+SK) | `pk = :d#e#env AND sk = REPIN` |

### Schema versions table

The `konductor-schema-versions` table is the canonical content store for every schema version in every environment. It exists because the EventBridge schema registry serves only the latest version's content and normalizes whitespace: replaying a promotion and detecting divergence both need historical, pre-normalization content. All side effects ride on the promotions table's stream, so this table has none.

| Attribute | Type | Key |
|-----------|------|-----|
| `pk` | String (`{domain}#{eventName}#{environment}`) | Partition key (PK) |
| `sk` | String (zero-padded version number) | Sort key (SK) |
| `content` | String | Not applicable |
| `contentHash` | String (SHA-256 of the canonical JSON) | Not applicable |
| `writtenBy` | String (`schema-admin` or a promotion id) | Not applicable |
| `writtenAt` | String (ISO 8601) | Not applicable |

| Pattern | Index | Key condition |
|---------|-------|---------------|
| Content of one version | Table (PK+SK) | `pk = :d#e#env AND sk = :version` |
| Version history of a schema in an environment | Table (PK) | `pk = :d#e#env` |
