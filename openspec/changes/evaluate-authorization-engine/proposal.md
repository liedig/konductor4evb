## Why

Nexus needs an authorization model that can express global platform roles, domain-scoped publishing and subscription rights, and schema-level entitlements consistently for Cognito-group principals and IAM-authenticated AWS account principals. A technical spike is needed before committing to an engine because DynamoDB-backed grants and Amazon Verified Permissions have materially different latency, cost, administration, testing, and operational trade-offs.

## What Changes

- Define a reproducible technical spike comparing DynamoDB-backed grants with Amazon Verified Permissions against identical authorization scenarios.
- Require measured evidence for latency, cost, auditability, local testability, policy administration, operational complexity, AWS account principal support, Cognito integration, hierarchy and inheritance, immediate revocation, and asynchronous command handling.
- Require the spike to declare reproducible workload volumes, measurement targets, and comparison rules before measurements, then use the evidence to recommend production SLOs rather than treating spike parameters as settled Nexus requirements.
- Produce a documented recommendation, evidence record, and decision rationale without implementing production authorization.
- Define acceptance criteria that prevent selecting either candidate without comparable evidence and explicit treatment of uncertainty.

## Capabilities

### New Capabilities

- `authorization-engine-evaluation`: Defines the scenarios, evidence, measurements, and decision record required to recommend an authorization engine for Nexus.

### Modified Capabilities

None.

## Impact

The spike evaluates future authorization behavior at the Nexus API, asynchronous SQS command, subscription, publishing, Cognito, IAM, and audit boundaries. This change produces planning and evaluation artifacts only; it does not modify application code, deployed infrastructure, APIs, canonical specs, dependencies, or production behavior.
