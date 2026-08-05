## Why

Nexus needs an authorization model that can express global platform roles, domain-scoped publishing and subscription rights, and schema-level entitlements consistently for Cognito-group principals and IAM-authenticated AWS account principals. DynamoDB-backed grants are the selected initial engine because they provide Nexus ownership of the enterprise group-to-resource mapping and the reverse indexes required for immediate subscription revocation. A technical spike is needed to validate that design and establish production SLOs before committing to its production implementation.

## What Changes

- Define a reproducible technical spike validating DynamoDB-backed grants against the full Nexus authorization scenario suite.
- Require measured evidence for latency, cost, auditability, local testability, policy administration, operational complexity, AWS account principal support, Cognito integration, hierarchy and inheritance, immediate revocation, and asynchronous command handling.
- Require the spike to declare reproducible workload volumes and measurement targets before measurements, then use the evidence to recommend production SLOs rather than treating spike parameters as settled Nexus requirements.
- Produce a documented readiness decision, evidence record, and rationale without implementing production authorization.
- Record Amazon Verified Permissions as a considered but rejected alternative, including the circumstances that would trigger reevaluation.

## Capabilities

### New Capabilities

- `authorization-engine-evaluation`: Defines the scenarios, evidence, measurements, and decision record required to recommend an authorization engine for Nexus.

### Modified Capabilities

None.

## Impact

The spike evaluates future authorization behavior at the Nexus API, asynchronous SQS command, subscription, publishing, Cognito, IAM, and audit boundaries. This change produces planning and evaluation artifacts only; it does not modify application code, deployed infrastructure, APIs, canonical specs, dependencies, or production behavior.
