## Why

Nexus needs a browser- and UI-compatible identity path without weakening the AWS-account identity used by existing automation and cross-account integrations. Establishing the Cognito and trusted-principal contracts now lets later authorization work add resource entitlements without coupling identity validation to an undecided policy engine.

## What Changes

- Add a dedicated Amazon Cognito user pool and OAuth/OIDC application foundation for interactive users, with standard Cognito identity and `cognito:groups` as the supported identity contract and a future federation seam.
- Add a REST API OAuth surface protected by a REQUEST Lambda authorizer that validates Cognito access tokens, maps controlled groups to the coarse global roles `platform-admin` and `platform-auditor`, preserves other trusted Cognito team-group identifiers, and supplies a compact trusted principal context without entitlements.
- Keep the IAM API surface separate and use the AWS account as its resource-binding identity; normalize both surfaces into a common trusted-principal contract for downstream authorization.
- Preserve direct API Gateway-to-SQS integrations. OAuth mapping templates inject trusted principal context, while queue consumers remain responsible for resource authorization; `202 Accepted` means only that a command was durably accepted.
- Make approval an OAuth-only operation and derive the decider from authenticated principal context. The Step Functions task token remains callback state, not caller authorization.
- Define safe authorizer caching and access-token revocation bounds without placing request-body, resource-level authorization, or complete entitlements in the authorizer.
- Keep AWS-native publishing, cross-account provisioning, and EventBridge delivery on IAM-based mechanisms outside this OAuth identity path.
- **BREAKING** Remove unauthenticated bearer task-token/email-link approval as an authorization path.
- **BREAKING** Stop accepting caller-asserted `decidedBy` as authoritative on the approval endpoint.
- **BREAKING** The existing IAM-only control-plane API contract becomes explicitly separated into IAM and OAuth surfaces, and approval is no longer available on the IAM surface.

## Capabilities

### New Capabilities

- `control-plane-access`: Interactive Cognito identity, separate OAuth and IAM API surfaces, global-role and trusted team-group propagation, account-granular IAM identity, trusted-principal normalization, and authorization-boundary contracts.

### Modified Capabilities

- `system-architecture`: Change the IAM-only REST API requirement to separate OAuth and IAM control-plane surfaces while preserving direct service integrations.
- `async-command-processing`: Propagate trusted principal context through direct API Gateway-to-SQS integrations and clarify that asynchronous acceptance is not authorization.
- `domain-management`: Require OAuth-authenticated approval, trusted decider identity, and task-token use only as workflow callback state.
- `domain-provisioning-workflow`: Require an authenticated platform approver independently of the Step Functions task token.
- `domain-owner-notifications`: Remove the task token/email content as a bearer authorization mechanism and direct approvers to the authenticated OAuth experience.

## Impact

- Affects the SAM control-plane stack, OpenAPI contracts, API Gateway REST APIs and mapping templates, Cognito resources, the REQUEST Lambda authorizer, shared principal models, queue consumers, approval handling, and approval notifications.
- Adds Cognito/JWT validation configuration and operational token-lifetime considerations.
- Preserves AWS-account identity for IAM authorization and AWS-native and cross-account paths, while retaining IAM principal ARN only as optional audit provenance, and preserves API Gateway-to-SQS durability semantics.
- Final resource-policy evaluation remains dependent on the authorization-engine spike; this change defines only the stable identity and principal contracts consumed by that future implementation.
