## MODIFIED Requirements

### Requirement: Separate OAuth and IAM API surfaces

Nexus SHALL expose separate REST API surfaces for OAuth-authenticated interactive users and AWS_IAM-authenticated callers. The IAM surface SHALL use the authenticated AWS account as its resource-authorization identity and MAY retain the IAM principal ARN only as audit provenance. A route exposed on both surfaces SHALL enforce the same operation contract and authorization semantics after trusted-principal normalization. AWS-native direct EventBridge publishing, cross-account provisioning, and EventBridge delivery SHALL remain IAM-based and outside the OAuth control-plane identity mechanism.

Binding administration SHALL be exposed on the OAuth surface only, because it is an interactive platform-administration function. The OAuth authorizer's resolution of the `platform-admin` global role from controlled Cognito groups SHALL remain independent of the grant engine's binding storage so that binding administration remains available when the bindings table is empty.

#### Scenario: Interactive caller uses the OAuth surface

- **WHEN** an interactive client invokes an OAuth control-plane route
- **THEN** the route SHALL require a valid Cognito access token through the Nexus REQUEST authorizer
- **AND THEN** IAM request signing SHALL NOT substitute for the OAuth credential on that route

#### Scenario: AWS caller uses the IAM surface

- **WHEN** an AWS automation client invokes an IAM control-plane route
- **THEN** API Gateway SHALL authenticate the request with AWS_IAM
- **AND THEN** the trusted principal SHALL use the caller's AWS account for resource authorization
- **AND THEN** the IAM principal ARN MAY be retained only as audit provenance

#### Scenario: Binding administration requires OAuth platform-admin

- **WHEN** a caller invokes a binding administration route
- **THEN** the route SHALL exist only on the OAuth surface
- **AND THEN** the caller SHALL hold the `platform-admin` global role resolved by the OAuth authorizer
- **AND THEN** the grant engine's binding storage SHALL NOT be consulted to establish the `platform-admin` role

#### Scenario: AWS-native data-plane integration operates

- **WHEN** a producer publishes directly through AWS-native EventBridge paths or Nexus provisions or delivers resources across accounts
- **THEN** those interactions SHALL continue to use IAM roles and resource policies
