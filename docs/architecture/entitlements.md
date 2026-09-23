# Entitlements and revocation

[Documentation home](../README.md)

Bindings authorize a principal to act for a domain. Entitlements govern which producer contracts a consumer domain can access.

> **Review pending:** [D05](../reference/documentation-discrepancies.md#d05) records conflicting or unverified guidance retained from the previous documentation. This reorganization does not resolve it.

## Entitlements

Where bindings answer "may this principal act for this domain?", **entitlements** answer "which producer event contracts may this consumer domain see?". They live in a second table, `konductor-authorization-entitlements`, because their revocation semantics differ: a binding gates nothing materialized, but revoking an entitlement can strand active subscriptions that must be converged.

| Attribute | Type | Key |
|-----------|------|-----|
| `consumerDomain` | String | Partition key (PK) |
| `grant` | String (`{producerDomain}#{scopePattern}`) | Sort key (SK) |
| `producerDomain` | String | GSI: `by-producer` (PK) |
| `entitlementId` | String | GSI: `by-entitlement` (PK) |
| `scopePattern` | String | Not applicable |
| `grantedBy` / `grantedAt` / `revokedAt` | Not specified | Audit provenance |
| `expiresAt` | Number (epoch, optional) | TTL—removal can lag expiry by days; expiry is also evaluated logically at read time |

The **scope pattern** is a single additive grammar reused by coverage checks, subscription authorization, and catalog expansion: `*` covers the producer's whole domain (including future schemas), an exact detail type covers every version of one schema (including future versions), and a `*` suffix covers a prefix family (including future members). `DynamoGrantEngine::covers()` evaluates coverage against ACTIVE entitlements; `list_entitlements()` feeds the catalog expansion.

**Revocation pipeline.** Revoke is a durable conditional delete in the admin transaction—effective at the *next decision* immediately. Converging already-materialized subscriptions is event-driven off the table's DynamoDB stream:

1. **Stream consumer** (`entitlement-stream-consumer`) claims each REMOVE image with a conditional idempotency-marker put (a replayed record is a no-op), enqueues a revocation message to the `EntitlementRevocationQueue` (DLQ after 3 receives), and forwards audit records to `konductor-bus`. The stream carries **change records**, never business events.
2. **Worker** (`entitlement-revocation-worker`) re-reads the consumer's remaining entitlements (fail-closed), finds candidate streams via exact, schema-prefix, or whole-partition queries narrowed by the revoked pattern, and disables the subscription's rules in every environment—the subscription record and its pinned version are retained so re-granting can re-enable.
3. **Sweep** (`entitlement-coverage-sweep`, hourly `rate(1 hour)`) scans the event-streams table, re-evaluates coverage per stream, and disables any ACTIVE stream that lost coverage—bounding out-of-band drift and covering missed fast-path deliveries. Drift disablements are alarmed; steady state is zero.

**Entitlement administration** mirrors binding administration: flat verbs, platform-admin-only, with a dual surface—OAuth for operators (`POST /entitlements`, `GET /entitlements`, `DELETE /entitlements/{entitlementId}`) and IAM for platform service roles under `IamOnlyAuthorizer`. Creates are deterministic-key conditioned (identical re-creates answer 200 with the stored entitlement); revokes are state-conditioned deletes; every mutation commits with its token-free `AuditRecord` in one `TransactWriteItems`.

## Related topics

- [Subscription materialization](subscriptions.md)
- [Access administration](../operations/access-administration.md#bootstrapping-role-bindings)
- [Entitlement decision](../decisions/add-subscriber-entitlements.md)
