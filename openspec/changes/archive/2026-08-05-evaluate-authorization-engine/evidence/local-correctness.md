# Local Correctness Evidence — DynamoDB grant-engine spike

> Spike artifact. Produced by `cargo run -p auth-engine-spike` against the in-memory grant adapter. No AWS credentials are used; cloud latency, cost, revocation, and asynchronous-handling measurements (tasks 4.x, 5.x) are deferred.

## 1. Frozen fixture integrity

- Manifest: `evidence/fixture-manifest.json`
- SHA-256 (computed): `ec4efb25c034159aafb041bc50061961dc0140c10380ab794ce9067547b441b0`
- Frozen hash (methodology.md): `ec4efb25c034159aafb041bc50061961dc0140c10380ab794ce9067547b441b0`
- Match: yes

## 2. Execution environment

- Adapter: `InMemoryGrantAdapter` (local, no AWS credentials)
- AWS Region (declared, not contacted): us-east-1
- Trusted Cognito issuer: `https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh`
- Sample sizes, workload profiles, provisional targets: declared in `methodology.md` (cloud measurement deferred)
- Secrets: none present; configuration retained with secrets removed

## 3. Correctness results

Scenarios: 13/13 passed. Steps: 34/34 passed.

| Scenario | Passed | Steps | Failing step |
|---|---|---|---|
| S01-platform-admin-allow | yes | 1 |  |
| S02-platform-auditor-read-allow-admin-deny | yes | 2 |  |
| S03-cognito-group-binding | yes | 1 |  |
| S04-domain-scoped-publisher-bound-and-unbound | yes | 2 |  |
| S05-domain-scoped-subscriber-bound-and-unbound | yes | 2 |  |
| S06-producer-domain-entitlement | yes | 3 |  |
| S07-exact-schema-entitlement | yes | 2 |  |
| S08-producer-growth-after-grant | yes | 4 |  |
| S09-additive-grants-union-and-removal | yes | 3 |  |
| S10-revocation-decision-and-subscription | yes | 2 |  |
| S11-cognito-group-and-aws-account-parity | yes | 4 |  |
| S12-unauthorized-non-disclosure | yes | 3 |  |
| S13-asynchronous-authorization | yes | 5 |  |

### Per-step detail

**S01-platform-admin-allow** — Global platform roles

- single-case: expected `"allow"`, observed `allow`, pass=true

**S02-platform-auditor-read-allow-admin-deny** — Global platform roles

- single-case: expected `"allow"`, observed `allow`, pass=true
- single-case: expected `"deny"`, observed `deny:AuditorRoleNotAuthorizedForMutation`, pass=true

**S03-cognito-group-binding** — Cognito group binding

- single-case: expected `"allow"`, observed `allow`, pass=true

**S04-domain-scoped-publisher-bound-and-unbound** — Domain-scoped publisher

- single-case: expected `"allow"`, observed `allow`, pass=true
- single-case: expected `"deny"`, observed `deny:PublisherNotBoundToDomain`, pass=true

**S05-domain-scoped-subscriber-bound-and-unbound** — Domain-scoped subscriber

- single-case: expected `"allow"`, observed `allow`, pass=true
- single-case: expected `"deny"`, observed `deny:SubscriberNotBoundToConsumerDomain`, pass=true

**S06-producer-domain-entitlement** — Producer-domain entitlement

- single-case: expected `"allow"`, observed `allow`, pass=true
- single-case: expected `"allow"`, observed `allow`, pass=true
- single-case: expected `"deny"`, observed `deny:NoApplicableEntitlement`, pass=true

**S07-exact-schema-entitlement** — Exact-schema entitlement

- single-case: expected `"allow"`, observed `allow`, pass=true
- single-case: expected `"deny"`, observed `deny:NoApplicableEntitlement`, pass=true

**S08-producer-growth-after-grant** — Producer growth after grant

- single-case: expected `"allow"`, observed `allow`, pass=true
- single-case: expected `"allow"`, observed `allow`, pass=true
- single-case: expected `"deny-as-unknown"`, observed `deny-as-unknown`, pass=true
- single-case: expected `"deny"`, observed `deny:NoApplicableEntitlement`, pass=true

**S09-additive-grants-union-and-removal** — Additive grants

- Both overlapping grants present: expected `"allow"`, observed `allow`, pass=true
- Remove exact-schema grant ent-ana-pay-cap-001: expected `"allow"`, observed `allow`, pass=true
- Remove producer-domain grant ent-ana-pay-full-001: expected `"deny"`, observed `deny:NoApplicableEntitlement`, pass=true

**S10-revocation-decision-and-subscription** — Revocation affects decisions and active subscriptions

- Revoke sole grant for risk-cons subscription sub-002: expected `"deny"`, observed `deny:NoApplicableEntitlement`, pass=true
- Revoke one of two overlapping grants for analytics-cons subscription sub-003: expected `"allow"`, observed `allow`, pass=true

**S11-cognito-group-and-aws-account-parity** — Cognito group and AWS account parity

- Cognito group bound to publisher on orders-prod: expected `"allow"`, observed `allow`, pass=true
- AWS account bound to publisher on orders-prod (equivalent decision): expected `"allow"`, observed `allow`, pass=true
- Assumed-role session within the same AWS account (ARN audit-only, same decision): expected `"allow"`, observed `allow`, pass=true
- Forged group context from untrusted issuer: expected `"deny"`, observed `deny:UntrustedIssuer`, pass=true

**S12-unauthorized-non-disclosure** — Unauthorized non-disclosure

- Unauthorized schema must be indistinguishable from unknown schema: expected `"deny-as-unknown"`, observed `deny-as-unknown`, pass=true
- Unknown schema (control case for indistinguishability): expected `"deny-as-unknown"`, observed `deny-as-unknown`, pass=true
- List filters unauthorized resources rather than revealing them as denied entries: expected `{"omittedSchemas":["payment-refunded","payment-disputed"],"visibleSchemas":["payment-captured"]}`, observed `allow`, pass=true

**S13-asynchronous-authorization** — Asynchronous authorization

- Cognito-group command denied after revocation before consume: expected `"deny"`, observed `deny:NoApplicableEntitlement`, pass=true
- IAM-authenticated AWS account command denied after revocation before consume: expected `"deny"`, observed `deny:SubscriberNotBoundToConsumerDomain`, pass=true
- Replay of already-handled command is denied: expected `"deny"`, observed `deny:DuplicateCommand`, pass=true
- Incomplete principal context is denied: expected `"deny"`, observed `deny:IncompletePrincipalContext`, pass=true
- Tampered principal context is denied: expected `"deny"`, observed `deny:TamperedPrincipalContext`, pass=true

## 4. Cloud-service semantic gaps (task 2.3)

The in-memory adapter implements the grant-table key model and the full decision/revocation/non-disclosure logic, but does NOT model the following DynamoDB cloud-service semantics. These are exercised by the DynamoDB-backed adapter (`src/dynamo.rs`, excluded from this local run) in the cloud integration phase:

- **Read consistency**: the local store has no consistency lag. Cloud decisions MUST use `ConsistentRead=true` on the grant table; GSIs are eventually consistent and serve administration/candidate lookup only.
- **GSI / subscription reverse-index lag**: a just-created subscription may be briefly absent from the consumer/producer GSI during revocation candidate lookup. Safe because subscription activation performs a final current-entitlement check before enabling EventBridge rules.
- **Throttling, capacity, retries**: the local adapter never throttles. Cloud runs must handle `ProvisionedThroughputExceededException` with backoff; retries are recorded in latency evidence.
- **SDK transport and cold start**: the local adapter is in-process. Cloud measurements include AWS SDK client initialization and network latency (cold-start-path samples in methodology.md).
- **Conditional writes / concurrent grant mutation**: the local adapter serializes via a `Mutex`. Cloud grant mutation needs optimistic concurrency or transactions; concurrent-change behavior is recorded under the policy-administration exercise (task 6.2).
- **Audit persistence**: the local adapter retains audit records in memory only. Cloud audit uses DynamoDB audit items and CloudTrail; audit reconstruction is exercised under task 6.1.
- **Revocation propagation timing**: the local adapter reconciles subscriptions synchronously and instantaneously. Cloud revocation timing (time to first denied decision vs complete subscription disablement) is measured separately in task 5.x.

## 5. Mandatory behavior status

- Correctness: all frozen scenarios pass against the in-memory adapter
- Non-disclosure: verified — S12a (unauthorized existing schema) and S08c (exact-schema consumer, producer-growth schema) both return `deny-as-unknown`, indistinguishable from the S12b unknown-schema control; S12c list filtering omits unauthorized schemas.
- Decision: local correctness satisfies mandatory correctness and non-disclosure behavior. Cloud latency, cost, revocation-propagation, and asynchronous-handling measurements (tasks 4.x, 5.x) and operational exercises (task 6.x) remain before the readiness decision (task 7.1).
