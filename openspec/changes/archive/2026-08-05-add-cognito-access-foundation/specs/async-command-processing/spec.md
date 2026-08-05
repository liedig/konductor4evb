## ADDED Requirements

### Requirement: Trusted principal propagation for queued commands

Every authenticated API surface that uses a direct API Gateway-to-SQS service integration SHALL inject a compact trusted principal context into the queued command. The mapping template SHALL construct this context only from API Gateway authentication or authorizer context and SHALL NOT trust identity, group, role, or account assertions from the request body. For OAuth, the queue envelope SHALL preserve the Cognito subject, mapped global roles, and trusted team-group identifiers. For IAM, it SHALL preserve the AWS account as `principalId` and resource-binding identity and MAY preserve the IAM principal ARN only as audit provenance. The envelope SHALL NOT include resource entitlements.

#### Scenario: OAuth command is enqueued

- **WHEN** an OAuth-authenticated request passes structural validation on an SQS-backed write route
- **THEN** the API Gateway mapping template SHALL enqueue the command through the existing direct SQS service integration
- **AND THEN** the command SHALL contain trusted OAuth subject, global-role, and team-group context derived from the REQUEST authorizer
- **AND THEN** caller-provided identity or role fields SHALL NOT override that context

#### Scenario: IAM command is enqueued

- **WHEN** an AWS_IAM-authenticated request passes structural validation on an SQS-backed write route
- **THEN** the API Gateway mapping template SHALL enqueue the command through the direct SQS service integration
- **AND THEN** the command SHALL use the authenticated AWS account as its trusted IAM `principalId` and resource-binding identity
- **AND THEN** an IAM principal ARN MAY be included only as audit provenance

#### Scenario: Consumer receives a normalized principal

- **WHEN** a queue consumer processes a command from either authenticated API surface
- **THEN** it SHALL receive the common trusted-principal envelope
- **AND THEN** OAuth team-group identifiers SHALL remain available to later resource-binding evaluation without being interpreted as entitlements by this capability
- **AND THEN** IAM authorization SHALL remain account-granular even when principal ARN audit provenance is present

### Requirement: Durable acceptance is not resource authorization

A `202 Accepted` response from an SQS-backed route SHALL mean only that the structurally valid command and trusted principal context were durably accepted by the queue. API Gateway and the OAuth authorizer SHALL NOT perform request-body or resource-level authorization for the queued command. The queue consumer SHALL perform resource authorization before applying a state change, using the common DynamoDB-backed authorization contract validated by the authorization-engine spike.

#### Scenario: Structurally valid but unauthorized command is accepted

- **WHEN** a structurally valid command is enqueued successfully but the principal lacks permission for the target resource
- **THEN** API Gateway SHALL return `202 Accepted` for durable queue acceptance
- **AND THEN** the queue consumer SHALL reject the command during resource authorization
- **AND THEN** no protected resource mutation SHALL occur

#### Scenario: Authorizer does not inspect command body

- **WHEN** the OAuth authorizer evaluates an SQS-backed request
- **THEN** it SHALL decide token and coarse route access without using body fields to authorize the target resource
- **AND THEN** the direct API Gateway-to-SQS integration SHALL remain in place
