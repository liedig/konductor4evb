## MODIFIED Requirements

### Requirement: Separate OAuth and IAM API surfaces

Konductor SHALL expose separate REST API surfaces for OAuth-authenticated interactive users and AWS_IAM-authenticated callers. The IAM surface SHALL use the authenticated AWS account as its resource-authorization identity and MAY retain the IAM principal ARN only as audit provenance. A route exposed on both surfaces SHALL enforce the same operation contract and authorization semantics after trusted-principal normalization. AWS-native direct EventBridge publishing, cross-account provisioning, and EventBridge delivery SHALL remain IAM-based and outside the OAuth control-plane identity mechanism.

API Gateway stages SHALL be named after platform environments (`dev`, `staging`, `prod`) instead of a static version stage. The stage name SHALL be available as `$context.stage` in mapping templates and request contexts. Publish and schema write operations SHALL use the stage to infer the target environment. Environment-agnostic operations (register, approve, list, describe) SHALL behave identically on any stage.

#### Scenario: Interactive caller uses the OAuth surface

- **WHEN** an interactive client invokes an OAuth control-plane route
- **THEN** the route SHALL require a valid Cognito access token through the Konductor REQUEST authorizer
- **AND THEN** IAM request signing SHALL NOT substitute for the OAuth credential on that route

#### Scenario: AWS caller uses the IAM surface

- **WHEN** an AWS automation client invokes an IAM control-plane route
- **THEN** API Gateway SHALL authenticate the request with AWS_IAM
- **AND THEN** the trusted principal SHALL use the caller's AWS account for resource authorization
- **AND THEN** the IAM principal ARN MAY be retained only as audit provenance

#### Scenario: Equivalent publisher routes use one resource decision

- **WHEN** equivalent OAuth and IAM principals invoke the same publisher operation for the same domain
- **THEN** both surfaces SHALL normalize their authenticated principal
- **AND THEN** both SHALL request the same action and domain resource decision from the DynamoDB-backed grant engine

#### Scenario: Environment stage selects target for publish and schema operations

- **WHEN** a publisher invokes `POST /publish` or schema write operations on the `prod` stage
- **THEN** the target environment SHALL be `prod`
- **AND THEN** the consumer or handler SHALL resolve environment-specific registries and buses for `prod`

#### Scenario: AWS-native data-plane integration operates

- **WHEN** a producer publishes directly through AWS-native EventBridge paths or Konductor provisions or delivers resources across accounts
- **THEN** those interactions SHALL continue to use IAM roles and resource policies
