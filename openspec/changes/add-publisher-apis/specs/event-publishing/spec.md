## ADDED Requirements

### Requirement: Domain-scoped synchronous publish API

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /domains/{domain}/environments/{environment}/events/{eventName}` for synchronous request processing where interactive and automation compatibility is required. Both route contracts SHALL accept the same business `data` request and use the same response semantics after normalizing their respective authenticated principals. The API SHALL invoke a dedicated synchronous publish Lambda that resolves schema and domain metadata, constructs the canonical envelope, calls EventBridge `PutEvents`, and returns the entry outcome before completing the HTTP request. This Lambda SHALL NOT act as a generic ingress processor or replace asynchronous API Gateway-to-SQS ingestion patterns.

#### Scenario: Event accepted by EventBridge

- **WHEN** an authorized publisher sends a valid event and EventBridge accepts its entry
- **THEN** the API SHALL return `202 Accepted`
- **AND THEN** the response SHALL identify the accepted EventBridge entry

#### Scenario: EventBridge rejects the entry

- **WHEN** EventBridge reports an error for the publish entry
- **THEN** the API SHALL return a non-success response describing the publication failure
- **AND THEN** the API SHALL NOT report the event as accepted

### Requirement: Publisher resource authorization

Before accessing domain or schema metadata or publishing, Nexus SHALL submit the normalized principal, publisher action, and target domain to the selected authorization engine. A `publisher` resource-role binding MAY bind an issuer-qualified Cognito team group or AWS-account principal to a domain, and a principal MAY have bindings for one or more domains. The `publisher` role SHALL NOT be interpreted as a global Cognito role, and a binding for one domain SHALL NOT authorize another domain.

#### Scenario: Team group binding authorizes its domain

- **WHEN** the caller belongs to a Cognito team group with a `publisher` binding for domain `orders`
- **THEN** the caller SHALL be authorized to publish events for `orders`

#### Scenario: AWS account binding authorizes its domain

- **WHEN** an IAM caller's normalized AWS account has a `publisher` binding for domain `orders`
- **THEN** the caller SHALL be authorized to publish events for `orders` through the IAM control-plane surface

#### Scenario: OAuth and IAM decisions are equivalent

- **WHEN** an OAuth team group and an AWS account have equivalent `publisher` bindings for domain `orders`
- **THEN** the OAuth and IAM publish routes SHALL request the same action and resource decision
- **AND THEN** both SHALL enforce the same allow or deny result

#### Scenario: Binding is domain-isolated

- **WHEN** the caller has a `publisher` binding for `orders` but requests publication for `payments`
- **THEN** the API SHALL return `403 Forbidden`
- **AND THEN** Nexus SHALL NOT resolve the schema or call EventBridge

#### Scenario: Global publisher claim does not confer access

- **WHEN** a caller has a Cognito claim or group name containing `publisher` but no Nexus `publisher` binding for the target domain
- **THEN** the API SHALL return `403 Forbidden`

### Requirement: Latest selected-environment schema resolution

For every publish request, Nexus SHALL derive the canonical schema name from the active domain namespace and event name, query the selected environment's platform-managed registry, and resolve its latest numeric schema version. Nexus SHALL use that resolved version for envelope stamping. The caller SHALL NOT choose a schema name or schema version.

#### Scenario: Selected environment controls version resolution

- **WHEN** event `OrderNotification` has latest version 5 in `dev` and version 3 in `prod`, and the caller publishes to `prod`
- **THEN** Nexus SHALL resolve the schema from the prod registry
- **AND THEN** the published envelope SHALL stamp numeric schema version `3`

#### Scenario: Schema is absent from selected environment

- **WHEN** the derived schema does not exist in the selected environment's registry
- **THEN** the API SHALL return `404 Not Found`
- **AND THEN** Nexus SHALL NOT call EventBridge

#### Scenario: Caller-supplied version is not accepted

- **WHEN** a publish request attempts to supply a schema name or schema version
- **THEN** the API SHALL reject the unsupported field
- **AND THEN** Nexus SHALL resolve identity and version from platform state

### Requirement: Publisher permissions exclude platform administration

The `publisher` role SHALL authorize only publisher schema operations and event publication for bound domains. It SHALL NOT authorize management of registries, event buses, resource policies, EventBridge rules, subscriber grants, publisher bindings, or platform authorization configuration.

#### Scenario: Publisher attempts an administrative operation

- **WHEN** a caller has only a `publisher` binding for the target domain
- **THEN** Nexus SHALL deny registry, bus, policy, rule, subscriber-grant, and authorization-administration operations
