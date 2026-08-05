# Capability: Authorization Engine Evaluation

## Purpose

The evaluation contract that selected Nexus's authorization engine: the common authorization scenario suite every candidate must answer, the measurement and evidence discipline required of the evaluation, and the evidence-based readiness decision for DynamoDB-backed grants (with Amazon Verified Permissions as the considered, rejected alternative).

## Requirements

### Requirement: DynamoDB grant-engine evaluation

The spike SHALL evaluate a DynamoDB-backed grant engine using the defined logical principal classes, resources, actions, grant fixtures, AWS Region, workload profiles, and expected decisions. Storage-specific setup SHALL be isolated from the scenario definitions, and any scenario that cannot be represented by the design SHALL be recorded as an unsupported result rather than silently changed or omitted.

#### Scenario: Scenario fidelity

- **WHEN** the evaluation suite is run
- **THEN** the DynamoDB evaluator SHALL receive the frozen scenario identifiers, identities, grants, resources, actions, and expected outcomes
- **AND THEN** the evidence SHALL identify every storage-specific translation or unsupported scenario

#### Scenario: Reproducible environment

- **WHEN** a reviewer examines an evaluation result
- **THEN** the evidence SHALL identify the DynamoDB configuration, AWS Region, execution time, dataset size, tool versions, and workload profile needed to reproduce it

### Requirement: Common authorization scenario suite

The spike SHALL exercise the DynamoDB-backed grant engine against the full Nexus authorization model: global platform roles, Cognito group bindings, domain-scoped publisher and subscriber roles, producer-domain and exact-schema entitlements, additive grants, revocation, Cognito-group and IAM-authenticated AWS account principals, non-disclosing denials, and asynchronous SQS command authorization.

#### Scenario: Global platform roles

- **WHEN** a principal has the `platform-admin` global role
- **THEN** the DynamoDB evaluator SHALL evaluate the principal against representative platform administration actions
- **AND WHEN** a principal has the `platform-auditor` global role
- **THEN** the DynamoDB evaluator SHALL allow representative platform read actions and deny representative mutation actions

#### Scenario: Cognito group binding

- **WHEN** an OAuth principal belongs to a Cognito group bound to a Nexus role
- **THEN** the DynamoDB evaluator SHALL derive the expected effective role and decisions from the trusted group binding

#### Scenario: Domain-scoped publisher

- **WHEN** a principal has the publisher role for one or more named producer domains
- **THEN** the DynamoDB evaluator SHALL allow publishing only to those domains and deny publishing to an unbound domain

#### Scenario: Domain-scoped subscriber

- **WHEN** a principal has the subscriber role for one or more named consumer domains
- **THEN** the DynamoDB evaluator SHALL allow subscription administration only on behalf of those consumer domains

#### Scenario: Producer-domain entitlement

- **WHEN** a consuming domain is entitled to an entire producer domain
- **THEN** the DynamoDB evaluator SHALL allow subscriptions from that consumer to every schema in the entitled producer domain
- **AND THEN** the entitlement SHALL cover schemas registered in the producer domain after the grant without entitlement mutation
- **AND THEN** the DynamoDB evaluator SHALL deny subscriptions to a producer domain without an applicable entitlement

#### Scenario: Exact-schema entitlement

- **WHEN** a consuming domain is entitled to an exact schema in a producer domain
- **THEN** the DynamoDB evaluator SHALL allow that schema and deny a different schema from the same producer domain
- **AND THEN** the entitlement SHALL NOT expand to schemas registered after the grant

#### Scenario: Producer growth after grant

- **WHEN** one consumer domain holds a producer-domain entitlement and another consumer domain holds exact-schema entitlements for the same producer domain
- **AND WHEN** the producer domain registers a new schema after those grants
- **THEN** the domain-wide consumer SHALL be allowed to discover and subscribe to the new schema without any entitlement mutation
- **AND THEN** the exact-schema consumer SHALL be denied access to the new schema until a covering entitlement is granted

#### Scenario: Additive grants

- **WHEN** multiple direct, group-derived, role-derived, domain-level, or schema-level grants apply to a principal and resource
- **THEN** the DynamoDB evaluator SHALL calculate the effective authorization as the union of applicable grants
- **AND THEN** removing one grant SHALL preserve access supplied independently by another applicable grant

#### Scenario: Revocation affects decisions and active subscriptions

- **WHEN** the final applicable entitlement for an active subscription is revoked
- **THEN** the DynamoDB evaluator SHALL deny subsequent authorization checks for that subscription
- **AND THEN** the DynamoDB design SHALL cause every no-longer-authorized active subscription materialization to be disabled
- **AND THEN** the evidence SHALL measure decision-revocation and subscription-disable propagation separately

#### Scenario: Cognito group and AWS account parity

- **WHEN** a Cognito-group principal and an IAM-authenticated AWS account principal are bound to equivalent logical permissions
- **THEN** the DynamoDB evaluator SHALL return equivalent decisions without granting either principal type broader implicit access
- **AND THEN** IAM role or user identity SHALL NOT change the resource-grant decision within the authenticated AWS account
- **AND THEN** an IAM principal ARN MAY be retained as audit provenance but SHALL NOT be used as the resource-grant key

#### Scenario: Unauthorized non-disclosure

- **WHEN** a principal requests a domain, schema, or subscription it is not authorized to discover
- **THEN** the DynamoDB design SHALL support a response that is indistinguishable from the corresponding unknown-resource response
- **AND THEN** list decisions SHALL filter unauthorized resources rather than reveal them as denied entries

#### Scenario: Asynchronous authorization

- **WHEN** an authorized Cognito-group or IAM-authenticated AWS account request is accepted into an SQS command queue and its final applicable grant is revoked before command handling
- **THEN** the DynamoDB design SHALL preserve a stable trusted principal identifier in the command context
- **AND THEN** the handler-side authorization check SHALL deny the command using current grants rather than the enqueue-time decision
- **AND THEN** the denial SHALL be treated as a deterministic command outcome rather than an SQS retry

### Requirement: Measurable evaluation evidence

The spike SHALL collect quantitative and qualitative evidence for latency, cost, auditability, local testability, policy administration, operational complexity, AWS account principal support, Cognito integration, hierarchy and inheritance, immediate revocation, and asynchronous handling. Raw observations, calculation assumptions, failed trials, and evidence sources SHALL be retained with the spike result. Before measurements begin, the spike evidence plan SHALL declare sample sizes, workload volumes, target throughput, measurement boundaries, DynamoDB configuration, and any provisional evaluation targets. These parameters SHALL be identified as spike methodology rather than settled Nexus production requirements.

#### Scenario: Latency benchmark

- **WHEN** decision latency is benchmarked
- **THEN** the DynamoDB evaluator SHALL be measured with the predeclared warm and cold-start-path sample sizes for each principal type
- **AND THEN** the result SHALL report sample count, error count, throughput, and p50, p95, and p99 end-to-end decision latency
- **AND THEN** the result SHALL assess the DynamoDB measurements against the provisional targets declared in the spike evidence plan and recommend evidence-based production latency and throughput SLOs

#### Scenario: DynamoDB cost projection

- **WHEN** DynamoDB cost is evaluated
- **THEN** the result SHALL project monthly cost at the low, expected, and high decision volumes declared in the spike evidence plan
- **AND THEN** it SHALL itemize request, storage, policy or grant administration, logging, monitoring, and revocation-processing costs using dated public prices and explicit workload assumptions

#### Scenario: Revocation benchmark

- **WHEN** immediate revocation is benchmarked over the predeclared number of independent trials
- **THEN** the result SHALL report p50, p95, maximum, and failure count from accepted revocation to the first denied decision
- **AND THEN** it SHALL report the same measures from accepted revocation to confirmed disablement of every affected active subscription
- **AND THEN** the result SHALL assess the DynamoDB measurements against the provisional targets declared in the spike evidence plan and recommend evidence-based production revocation and subscription-disablement SLOs

#### Scenario: Operational evidence

- **WHEN** auditability, local testability, policy administration, operational complexity, AWS account principal support, Cognito integration, hierarchy and inheritance, or asynchronous handling is evaluated
- **THEN** each criterion SHALL include a reproducible exercise, observed result, required AWS services and operator steps, failure behavior, and evidence reference for the DynamoDB design

### Requirement: Evidence-based DynamoDB readiness decision

The spike SHALL publish a readiness decision for DynamoDB-backed grants before the next implementation change. Correct scenario behavior and unauthorized non-disclosure SHALL be mandatory. Before measurements begin, the spike evidence plan SHALL declare a readiness rubric explaining how all evaluation criteria will inform production readiness without representing arbitrary numerical weights as Nexus product priorities. Unsupported or unmeasured behavior SHALL remain explicit and SHALL NOT be inferred as successful. The readiness decision SHALL include the DynamoDB result, AVP as the rejected alternative, evidence, recommended production SLOs, material trade-offs, assumptions, residual risks, and conditions that would require reevaluation.

#### Scenario: DynamoDB satisfies mandatory behavior

- **WHEN** the DynamoDB design satisfies correctness and unauthorized non-disclosure
- **THEN** the readiness decision SHALL confirm it for production implementation unless the evidence identifies an explicit production-blocking constraint
- **AND THEN** any production-blocking constraint SHALL be documented with the remediation required before implementation

#### Scenario: DynamoDB has a production-blocking constraint

- **WHEN** the DynamoDB design satisfies correctness and unauthorized non-disclosure but the evidence identifies a production-blocking constraint
- **THEN** the readiness decision SHALL document the remediation required before production implementation

#### Scenario: DynamoDB fails mandatory behavior

- **WHEN** the DynamoDB design fails correctness or unauthorized non-disclosure
- **THEN** the readiness decision SHALL identify the smallest explicitly scoped remediation
- **AND THEN** production implementation SHALL be conditional on proving that remediation

#### Scenario: Reviewable decision

- **WHEN** the spike is complete
- **THEN** an independent reviewer SHALL be able to trace every mandatory behavior result, criterion assessment, recommended SLO, and readiness claim to retained evidence
- **AND THEN** the review SHALL confirm that the DynamoDB design was evaluated without changing common scenarios or the declared readiness rubric after results were observed
