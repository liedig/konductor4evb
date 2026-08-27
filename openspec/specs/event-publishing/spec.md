# Capability: Event Publishing

## Purpose

How producers publish events to a domain without knowing EventBridge internals: a flat store-and-forward API that queues requests durably, authorizes the publisher against domain bindings, resolves the stage-selected environment's latest schema version, stamps the canonical envelope with immutable publish metadata, and delivers to the domain's event bus with retries.

## Requirements

### Requirement: Flat store-and-forward publish API

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /publish` as a flat verb endpoint where the target domain is in the request body, not the URL path. The request SHALL accept `domain`, `eventName`, `data`, and an optional `idempotencyKey`. The API SHALL route through SQS for store-and-forward durability: API Gateway pushes the message to SQS and returns `202 Accepted` with the SQS message ID as `requestId`. A publish consumer Lambda SHALL read the message, resolve the schema, construct the canonical envelope, and call EventBridge `PutEvents`. If `PutEvents` fails, SQS SHALL retry with the same message attributes, preserving the idempotency key and published timestamp.

#### Scenario: Event queued for durable delivery

- **WHEN** an authorized publisher sends a valid publish request
- **THEN** the API SHALL return `202 Accepted` with the SQS message ID as `requestId`
- **AND THEN** the event SHALL be durably queued for delivery

#### Scenario: PutEvents failure triggers SQS retry

- **WHEN** the publish consumer calls `PutEvents` and EventBridge rejects the entry
- **THEN** the SQS message SHALL NOT be deleted
- **AND THEN** SQS SHALL retry delivery with the same message attributes, idempotency key, and published timestamp

#### Scenario: Event is never lost after queue acceptance

- **WHEN** a publish request has been accepted by SQS
- **THEN** the event SHALL survive downstream failures and be retried until delivery or DLQ exhaustion

### Requirement: Environment inferred from API stage

The publish and schema write APIs SHALL infer the target environment from the API Gateway stage name (`$context.stage`). Stages SHALL be named after platform environments (`dev`, `staging`, `prod`). The VTL mapping template SHALL inject the stage name as an SQS message attribute named `Environment`. The publish consumer Lambda SHALL read the `Environment` attribute to select the environment-specific schema registry and event bus. The caller SHALL NOT specify an environment field in the request body.

#### Scenario: Stage determines target environment

- **WHEN** a publisher sends `POST /publish` to the `prod` stage for domain `orders`
- **THEN** the consumer SHALL resolve schemas from `{org}-prod-orders-schema-registry`
- **AND THEN** the consumer SHALL publish to `{org}-prod-orders-bus`

#### Scenario: Same operation on different stages targets different environments

- **WHEN** a publisher sends `POST /publish` to the `dev` stage and then to the `prod` stage for the same domain and event
- **THEN** each invocation SHALL target the respective environment's registry and bus independently

#### Scenario: Environment-agnostic operations ignore the stage

- **WHEN** a caller invokes register, approve, list, or describe operations on any stage
- **THEN** the stage SHALL NOT affect the operation's behavior

### Requirement: Publisher resource authorization

Before accessing domain or schema metadata or publishing, Konductor SHALL submit the normalized principal, publisher action, and target domain to the DynamoDB-backed grant engine. A `publisher` resource-role binding MAY bind an issuer-qualified Cognito team group or AWS-account principal to a domain, and a principal MAY have bindings for one or more domains. The `publisher` role SHALL NOT be interpreted as a global Cognito role, and a binding for one domain SHALL NOT authorize another domain.

#### Scenario: Team group binding authorizes its domain

- **WHEN** the caller belongs to a Cognito team group with a `publisher` binding for domain `orders`
- **THEN** the caller SHALL be authorized to publish events for `orders`

#### Scenario: AWS account binding authorizes its domain

- **WHEN** an IAM caller's normalized AWS account has a `publisher` binding for domain `orders`
- **THEN** the caller SHALL be authorized to publish events for `orders` through the IAM control-plane surface

#### Scenario: Binding is domain-isolated

- **WHEN** the caller has a `publisher` binding for `orders` but requests publication for `payments`
- **THEN** the consumer SHALL deny authorization
- **AND THEN** the consumer SHALL NOT resolve the schema or call EventBridge

#### Scenario: Global publisher claim does not confer access

- **WHEN** a caller has a Cognito claim or group name containing `publisher` but no Konductor `publisher` binding for the target domain
- **THEN** the consumer SHALL deny authorization

### Requirement: Latest stage-selected-environment schema resolution

For every publish request, the consumer Lambda SHALL derive the canonical schema name from the active domain namespace and event name, query the stage-selected environment's platform-managed registry, and resolve its latest numeric schema version. The consumer SHALL use that resolved version for envelope stamping. The caller SHALL NOT choose a schema name or schema version.

#### Scenario: Stage-selected environment controls version resolution

- **WHEN** event `OrderNotification` has latest version 5 in `dev` and version 3 in `prod`, and the caller publishes to the `prod` stage
- **THEN** the consumer SHALL resolve the schema from the prod registry
- **AND THEN** the published envelope SHALL stamp numeric schema version `3`

#### Scenario: Schema is absent from stage-selected environment

- **WHEN** the derived schema does not exist in the stage-selected environment's registry
- **THEN** the consumer SHALL NOT call EventBridge
- **AND THEN** the message SHALL be treated as a processing failure

#### Scenario: Caller-supplied version is not accepted

- **WHEN** a publish request attempts to supply a schema name or schema version
- **THEN** the API SHALL reject the unsupported field
- **AND THEN** Konductor SHALL resolve identity and version from platform state

### Requirement: Publisher permissions exclude platform administration

The `publisher` role SHALL authorize only publisher schema operations and event publication for bound domains. It SHALL NOT authorize management of registries, event buses, resource policies, EventBridge rules, subscriber grants, publisher bindings, or platform authorization configuration.

#### Scenario: Publisher attempts an administrative operation

- **WHEN** a caller has only a `publisher` binding for the target domain
- **THEN** Konductor SHALL deny registry, bus, policy, rule, subscriber-grant, and authorization-administration operations
