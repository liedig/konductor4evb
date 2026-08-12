## Context

Konductor currently authenticates API callers with IAM and performs limited ownership checks in handlers. Planned OAuth access, platform roles, domain-scoped publisher and subscriber roles, and producer/schema entitlements require a shared authorization decision model. The selected model must also work after an API request crosses the SQS boundary and must revoke rights quickly enough to stop both new commands and already-materialized subscriptions.

This change is a technical spike, not a production authorization implementation. It validates a purpose-built grant model stored in DynamoDB and evaluated by Konductor. DynamoDB is the selected initial engine because Konductor needs to own enterprise group-to-resource mappings and maintain reverse indexes for immediate subscription revocation.

The evaluation fixes common scenario fixtures, mandatory correctness and non-disclosure behavior, workload profiles, and measurement procedures before results are collected. The spike runs in an isolated AWS sandbox and retains all reports and evidence under this change directory. Amazon Verified Permissions (AVP) is a considered, rejected alternative, not a candidate harness in this spike.

## Goals / Non-Goals

**Goals:**

- Prove that DynamoDB-backed grants can represent and correctly decide the complete common scenario suite.
- Collect reproducible latency, cost, revocation, security, testability, administration, and operations evidence for the DynamoDB design.
- Exercise Cognito-group and IAM-authenticated AWS account principals through one engine-neutral authorization contract.
- Exercise authorization at synchronous API boundaries and at asynchronous SQS handling boundaries.
- Measure revocation through denial and disablement of affected active subscriptions.
- Produce one reviewable readiness decision with the rejected AVP alternative, trade-offs, risks, and reevaluation triggers.

**Non-Goals:**

- Implement production API authorizers, grant administration APIs, policy stores, reconciliation workers, or subscription changes.
- Change existing Konductor authorization behavior or canonical capability specifications.
- Select the final production data model, Cedar schema, API contract, deployment topology, or migration plan beyond what is necessary to evaluate feasibility.
- Benchmark EventBridge data-plane event delivery; authorization applies to control-plane and publish API requests.
- Compare authorization products other than DynamoDB-backed grants and AVP.

## Decisions

### Use a DynamoDB authorization contract

The harness will define a versioned fixture manifest containing:

- `authorizationSubject`: either a trusted Cognito issuer/group or an authenticated AWS account ID;
- `auditProvenance`: optional Cognito subject or IAM principal ARN that never selects resource grants;
- `action`: a closed set of representative platform, publish, subscribe, and discover operations;
- `resource`: platform, domain, schema, or subscription identifiers with producer/consumer relationships;
- `context`: authentication method and command metadata needed by the scenario;
- grants and expected `allow` or `deny` decisions.

The DynamoDB evaluator translates the manifest into grant items and returns only a decision, reason category, evaluation timestamp, and trace reference. The manifest prevents storage-specific concepts from changing expected behavior.

Alternative considered: Amazon Verified Permissions (AVP), with Konductor identities and resources represented as Cedar entities and grants represented as policies. Rejected for the initial engine because AVP would still require Konductor-owned mappings and reverse indexes to identify subscriptions affected by entitlement revocation, adding a second authorization representation without removing the reconciliation responsibility.

### Model grants as additive and make default denial explicit

The shared fixtures contain AWS-account bindings, Cognito-group bindings, global role bindings, role-to-domain bindings, and consumer-to-producer/schema entitlements. Applicable allows form a union; absence of an applicable allow denies access. The spike will not introduce explicit deny grants because deny precedence is a separate product decision and is not required by the agreed scenarios.

Producer-domain entitlements are dynamic over current registry contents: a schema registered after the grant is covered without entitlement mutation. Exact-schema entitlements are a fixed enumeration and do not expand on producer growth.

The DynamoDB evaluator will use the smallest representative single-table grant/index shape that can resolve applicable bindings without scans. Evidence must identify denormalization and hierarchy maintenance required to preserve the contract.

### Use a representative grant-table model

The spike harness stores authorization state in a dedicated grant table with three item types:

- **RoleBinding**: `PK = PRINCIPAL#{cognito-group|aws-account}#{issuer-qualified group id | account id}`, `SK = ROLE#{role}#DOMAIN#{domain}`. Carries binding identifier, state, creation metadata, and trusted actor.
- **Entitlement**: `PK = CONSUMER#{consumerDomain}`, `SK = ENT#PRODUCER#{producerDomain}` for producer-domain grants and `SK = ENT#PRODUCER#{producerDomain}#SCHEMA#{schemaName}` for exact-schema grants.
- **AuditRecord**: `PK = GRANT#{grantId}#AUDIT`, `SK = TS#{timestamp}`.

Subscriptions are not grant-table items. They belong to the subscription bounded context and live in their own table with their own lifecycle, write paths, and TTL retention. The contract revocation needs from that store is one compound access path: given a consumer domain and producer domain, return candidate subscriptions. The spike models this as a GSI on the subscription table with `PK = CONSUMER#{consumerDomain}#PRODUCER#{producerDomain}` and `SK = SCHEMA#{schemaName}#SUB#{subscriptionId}`.

Two GSIs on the grant table support administration without scans: GSI1 by domain or producer scope, and GSI2 by item type for platform-wide enumeration.

Every required access pattern resolves as a key-condition query. Publisher or subscriber authorization reads the caller's exact binding key once per candidate principal. Entitlement coverage reads `CONSUMER#{consumerDomain}` with `SK begins_with ENT#PRODUCER#{producerDomain}`, returning the producer-domain item and exact-schema items in one query. Catalog expansion reads all entitlement items for a consumer domain. Revocation finds candidate subscriptions through the subscription table's consumer/producer GSI, then re-evaluates each against current entitlements in the grant table.

Coverage is computed at decision time rather than materialized. This keeps producer-domain grants dynamic over registry growth: the grant names the producer domain, never a schema list.

Subscriptions are indexed in their own store by consumer/producer scope, never by the grant that enabled them. Under additive semantics any current grant may cover a subscription, so revocation re-evaluates candidates against all remaining grants instead of following stored grant associations.

Authorization decisions use strongly consistent reads on the grant table. GSIs are eventually consistent, so they serve administration and candidate lookup only and never feed allow decisions. GSI lag on the subscription reverse index can briefly miss a just-created subscription during revocation; this is safe because subscription activation and enable perform a final current-entitlement check before enabling EventBridge rules.

Global platform roles are not stored in this table. The OAuth authorizer resolves `platform-admin` and `platform-auditor` from controlled Cognito group names; the grant engine evaluates resource roles and entitlements. Fixtures still exercise global-role scenarios to prove the decision contract composes with authorizer-supplied roles.

Alternative considered: co-locating subscription link items in the grant table for single-table harness simplicity. Rejected because subscriptions belong to the subscription bounded context with lifecycle and write paths owned by subscription workflows; a second copy in the grant table would need synchronization on every subscription mutation. The spike records the access patterns as the contract; physical layout beyond the grant/subscription split is a production decision.

### Compare Cognito groups with AWS account principals

Cognito authorization subjects are derived from a trusted issuer and its authenticated group claims. IAM authorization subjects are the authenticated AWS account IDs supplied by the API boundary. IAM roles, users, and assumed-role sessions within the same account do not receive separate Konductor resource grants. Their principal ARN may be retained as audit provenance, but the DynamoDB evaluator must not use it to select domain, schema, publisher, or subscriber grants.

The parity scenarios bind a Cognito group and an AWS account to equivalent logical permissions and expect identical decisions. The spike records the trusted boundary context required before each engine call so that AVP is not credited with authentication it does not perform and DynamoDB is not penalized for equivalent boundary work.

Alternative considered: grant Konductor resources to individual IAM role or user ARNs. Rejected because the agreed IAM authorization granularity is AWS account identity; finer IAM identity remains an audit concern and can continue to be governed within the owning account's IAM policies.

### Reauthorize asynchronous commands with current grants

An accepted command fixture carries the authorization subject type, the trusted Cognito issuer/group context or AWS account ID, optional Cognito subject or IAM principal ARN audit provenance, request identifier, action, and resource coordinates. It never stores a bearer token or treats the enqueue-time authorization result as durable authority. The handler reconstructs the authorization request and checks current grants immediately before side effects. DynamoDB grant lookup keys IAM resource grants only by AWS account ID.

The revocation trial pauses a queued command, removes its final applicable grant, waits for the revocation operation to be accepted, and then releases the command. A deterministic denial is recorded as a failed command outcome and acknowledged rather than retried. Tampered, incomplete, or untrusted principal context is denied.

Alternative considered: authorize only before enqueue. Rejected because a queued command could execute after revocation. Carrying OAuth tokens in SQS was also rejected because it creates secret-handling and token-expiry problems without guaranteeing current authorization.

### Evaluate revocation as decision invalidation plus subscription reconciliation

The DynamoDB grant engine does not itself disable EventBridge rules. The spike therefore includes this revocation contract:

1. accept a grant or entitlement removal;
2. make new decisions reflect the removal;
3. identify active subscriptions that depended on the removed authorization;
4. re-evaluate each affected subscription using remaining additive grants;
5. disable every subscription with no remaining authorization;
6. emit an auditable result.

The trial dataset includes subscriptions authorized by exactly one grant and subscriptions covered by overlapping grants. Decision propagation and complete materialization disablement are timed independently. The DynamoDB reverse-index mechanism for finding affected subscriptions must be documented and included in cost and operations evidence.

### Use frozen correctness cases and predeclared benchmark procedures

Before collecting results, the harness fixtures and expected outcomes are frozen and hashed. Every scenario runs first as a correctness trial. Any incorrect allow, incorrect deny, cross-tenant disclosure, changed fixture, or omitted scenario fails mandatory correctness or non-disclosure behavior.

Before measurements, the spike evidence plan declares warm and cold-path sample sizes, workload volumes, target throughput, Region, client timeouts, retry policy, dataset, and sequential/concurrent profiles. These values are spike methodology, not production requirements. Warm measurements exclude one-time fixture setup but include the complete authorization evaluator call. Cold-path measurements run in newly initialized evaluator processes and include client initialization. Reports include p50, p95, p99, throughput, errors, retries, and throttles. The final decision uses these results to recommend production latency and throughput SLOs.

The evidence plan also declares the number of independent revocation trials and provisional targets before execution. Revocation trials use no authorization decision cache. Reports distinguish time to first denied decision from time to complete disablement of affected subscriptions, and the decision record recommends production SLOs from the observed distributions and operational requirements.

### Project cost from declared workload profiles

The evidence plan declares low, expected, and high monthly decision volumes before measurements. The report calculates cost for those profiles using dated public prices for the selected Region. It itemizes authorization requests, DynamoDB reads/writes/storage or AVP operations/policy storage, revocation processing, audit logs, metrics, and supporting resources. Assumptions include principal mix, read/write ratio, grant count, subscription fan-out, item sizes, consistency mode, retries, and log retention. One-time development cost is not converted into a speculative dollar amount; operator steps and custom code are captured under operational complexity.

Alternative considered: compare list prices for only DynamoDB reads and AVP authorization calls. Rejected because grant mutation, reconciliation, audit, and observability costs can materially alter the result.

### Collect structured qualitative evidence

Each non-benchmark criterion has a prescribed exercise:

- **Auditability:** reconstruct who changed a grant, what decision occurred, and why from retained records without exposing sensitive tokens.
- **Local testability:** run the common correctness suite without AWS credentials, then compare it with the cloud integration suite and document semantic gaps.
- **Policy administration:** create, review, update, revoke, and enumerate each binding and entitlement type, including concurrent changes.
- **Operational complexity:** inventory services, deployable units, alarms, quotas, retries, failure modes, backup/recovery steps, and operator actions.
- **AWS account principal support:** apply domain and schema grants by authenticated AWS account ID, verify equivalent decisions across different IAM role/user provenance from that account, and preserve principal ARN only in audit evidence.
- **Cognito integration:** map trusted users and groups, reject forged/untrusted group context, and update a group binding.
- **Hierarchy and inheritance:** evaluate global roles, group-to-role, role-to-domain, producer-domain-to-schema, exact-schema, and overlapping grants.
- **Immediate revocation:** execute the timed denial and active-subscription reconciliation trials.
- **Asynchronous handling:** execute enqueue, delayed handling, revocation-before-consume, replay, and tampered-context cases.

Every exercise records setup, steps, observed result, failure behavior, required custom logic, operator effort, and an evidence reference. Unsupported behavior is retained as evidence rather than approximated away.

### Apply a predeclared readiness rubric

Correctness and unauthorized non-disclosure are mandatory. Before measurements, the spike evidence plan declares how the remaining criteria determine production readiness, which observations constitute support or concern, how uncertainty is represented, and which production constraints could block implementation. The rubric covers latency, cost, auditability, local testability, policy administration, operational complexity, AWS account principal support, Cognito integration, hierarchy and inheritance, immediate revocation, and asynchronous handling.

The readiness record presents measurements, qualitative observations, unsupported behavior, uncertainty, and trade-offs. Two reviewers independently verify evidence references and challenge readiness against the declared rubric. The record determines whether the DynamoDB design is ready for production implementation and proposes production latency, throughput, revocation, and subscription-disablement SLOs grounded in the measurements. If it fails mandatory behavior, the result is conditional on the smallest evidenced remediation needed to do so.

### Record explicit reevaluation triggers

The readiness decision records the conditions that reopen the rejected AVP alternative or a third engine:

- decision p99 latency or revocation propagation misses the recommended SLO after documented tuning of the representative grant model;
- maintaining the subscription reverse index or additive re-evaluation proves operationally unsustainable at evidenced volumes;
- a product requirement for explicit deny policies, delegation, or condition logic emerges that the additive grant model cannot express;
- enterprise identity requirements exceed issuer-qualified group and AWS-account bindings, such as per-user resource grants at scale.

### Keep the spike result reviewable and disposable

The spike will retain the frozen fixture manifest and hash, DynamoDB configuration with secrets removed, raw measurement output, cost worksheet, exercise notes, evidence matrix, recommended production SLOs, and final readiness decision in a dedicated evidence area under this change. Temporary sandbox resources are tagged for the spike and destroyed after evidence is captured. The final report links every mandatory result and readiness claim to evidence and records DynamoDB as the selected initial engine, AVP as the rejected alternative, assumptions, residual risks, and reevaluation triggers.

The spike implementation is disposable. A later OpenSpec change must define production authorization behavior and architecture using the recommendation; no harness resource or prototype becomes production infrastructure by default.

## Risks / Trade-offs

- **[Synthetic workload differs from production traffic]** → Publish the workload model, run the predeclared sensitivity profiles, and retain raw data so assumptions can be recalculated.
- **[The DynamoDB evaluator accidentally changes authorization semantics]** → Freeze and hash one fixture manifest, run mutation tests, and review all translations.
- **[AWS service noise distorts latency]** → Use the same Region and time windows, report distributions and retries, repeat anomalous trials, and retain raw samples.
- **[Revocation timing is confused with subscription workflow latency]** → Report decision invalidation and subscription disablement as separate measurements.
- **[Qualitative evidence interpretation remains subjective]** → Freeze the readiness rubric, use two independent reviewers, cite evidence, and record disagreements and resolutions.
- **[Cost estimates become stale]** → Date every price and preserve formulas and workload assumptions rather than only final totals.
- **[Prototype security shortcuts are mistaken for production design]** → Label all resources and reports as spike-only, exclude secrets, destroy sandbox resources, and require a separate production change.
- **[The DynamoDB design does not satisfy mandatory behavior]** → Produce a conditional readiness decision with the smallest scoped remediation and require proof before production implementation.

## Migration Plan

There is no production migration in this change.

1. Create isolated, tagged sandbox resources for the DynamoDB evaluator.
2. Run and retain the frozen correctness, benchmark, revocation, and operational exercises.
3. Review evidence and publish the decision record.
4. Destroy sandbox resources and verify cleanup.
5. Open a separate OpenSpec change for the DynamoDB production architecture; do not reuse spike resources implicitly.

Rollback consists of stopping the spike, deleting its sandbox resources, and retaining the incomplete evidence with a clear status. Existing Konductor behavior is unaffected.

## Open Questions

The spike must record, but does not need to settle, the expected production authorization request rate, an organization-specific monthly budget, and whether explicit deny policies will be required later. These unknowns are sensitivity and reevaluation inputs; they do not alter the frozen common scenarios or declared readiness rubric.
