## MODIFIED Requirements

### Requirement: Separate OAuth and IAM API surfaces

Nexus SHALL expose separate REST API surfaces for OAuth-authenticated interactive users and AWS_IAM-authenticated callers. The IAM surface SHALL use the authenticated AWS account as its resource-authorization identity and MAY retain the IAM principal ARN only as audit provenance. A route SHALL NOT accept both mechanisms as interchangeable credentials. Publisher schema and event routes SHALL be exposed on both surfaces where interactive and automation compatibility is required, with identical request, response, and resource-authorization semantics after trusted-principal normalization. AWS-native direct EventBridge publishing, cross-account provisioning, and EventBridge delivery SHALL remain IAM-based and outside the OAuth control-plane identity mechanism.

#### Scenario: Interactive caller uses the OAuth surface

- **WHEN** an interactive client invokes an OAuth control-plane route
- **THEN** the route SHALL require a valid Cognito access token through the Nexus REQUEST authorizer
- **AND THEN** IAM request signing SHALL NOT substitute for the OAuth credential on that route

#### Scenario: AWS caller uses the IAM surface

- **WHEN** an AWS automation client invokes an IAM control-plane route
- **THEN** API Gateway SHALL authenticate the request with AWS_IAM
- **AND THEN** the trusted principal SHALL use the caller's AWS account for resource authorization
- **AND THEN** the IAM principal ARN MAY be retained only as audit provenance

#### Scenario: Equivalent publisher routes use one resource decision

- **WHEN** equivalent OAuth and IAM principals invoke the same publisher operation for the same domain
- **THEN** both surfaces SHALL normalize their authenticated principal
- **AND THEN** both SHALL request the same action and domain resource decision from the selected authorization engine

#### Scenario: AWS-native data-plane integration operates

- **WHEN** a producer publishes directly through AWS-native EventBridge paths or Nexus provisions or delivers resources across accounts
- **THEN** those interactions SHALL continue to use IAM roles and resource policies
- **AND THEN** they SHALL NOT depend on Cognito user identity

### Requirement: Trusted principal normalization

Both API surfaces SHALL normalize authenticated identity into a trusted principal context with a stable mechanism discriminator, principal identifier, and mechanism-specific provenance. OAuth principals SHALL include the Cognito issuer, subject, trusted `cognito:groups`, and mapped global roles. IAM principals SHALL use the AWS account identifier as `principalId` and MAY include the IAM principal ARN only as audit provenance. Downstream components SHALL consume this trusted context rather than request-body identity assertions.

#### Scenario: OAuth principal is normalized

- **WHEN** the OAuth authorizer allows a request
- **THEN** downstream integrations SHALL receive a trusted principal identifying the OAuth mechanism, Cognito issuer, subject, trusted team-group memberships, and mapped global roles
- **AND THEN** the context SHALL be derived only from validated access-token claims

#### Scenario: IAM principal is normalized

- **WHEN** API Gateway authenticates a request on the IAM surface
- **THEN** downstream integrations SHALL receive a trusted principal identifying the IAM mechanism, AWS account, and IAM principal
- **AND THEN** resource authorization SHALL distinguish AWS accounts but SHALL NOT create role-, user-, or session-level Nexus grants within one account

#### Scenario: Caller cannot assert binding identity

- **WHEN** a request body contains a Cognito group, AWS account, role, or other principal assertion
- **THEN** Nexus SHALL ignore or reject the assertion as specified by the route contract
- **AND THEN** authorization SHALL use only the trusted normalized principal context

## ADDED Requirements

### Requirement: Platform-admin publisher binding administration

The OAuth platform-administration surface SHALL provide platform-admin operations to create, list, and revoke `publisher` bindings for a domain. The administration contract SHALL support issuer-qualified Cognito team-group principals and AWS-account principals. A principal MAY be bound to multiple domains through distinct domain bindings. Nexus SHALL NOT provide a global `publisher` role or infer publisher access from an unbound group name, account, or claim.

#### Scenario: Platform admin binds a Cognito team group

- **WHEN** an OAuth caller with the `platform-admin` global role binds an issuer-qualified Cognito team group to the `publisher` role for domain `orders`
- **THEN** Nexus SHALL create a domain-scoped binding for that group and domain
- **AND THEN** the same group SHALL remain unauthorized for an unbound domain

#### Scenario: Platform admin binds an AWS account

- **WHEN** an OAuth caller with the `platform-admin` global role binds AWS account `123456789012` to the `publisher` role for domain `orders`
- **THEN** Nexus SHALL create a domain-scoped binding whose principal type is AWS account
- **AND THEN** IAM callers normalized to that account SHALL be eligible for the same publisher decisions as an equivalently bound Cognito team group

#### Scenario: Non-admin attempts binding administration

- **WHEN** a caller without the `platform-admin` global role attempts to create, list, or revoke publisher bindings
- **THEN** Nexus SHALL deny the request
- **AND THEN** no binding state SHALL be disclosed or changed

#### Scenario: Publisher cannot administer bindings

- **WHEN** a caller has a `publisher` binding but lacks the `platform-admin` global role
- **THEN** the binding SHALL NOT authorize publisher-binding administration

### Requirement: Production publisher binding persistence

Publisher bindings SHALL be durably persisted through the production adapter for the authorization engine selected by `evaluate-authorization-engine`. Each binding SHALL retain a stable binding identifier, role, domain resource, typed principal, creation time, and trusted platform-admin actor. Successful create and revoke responses SHALL mean the mutation is persisted, auditable, and reflected by subsequent current-state authorization decisions. Binding evaluation SHALL be additive and default-deny.

#### Scenario: Binding survives service restart

- **WHEN** a publisher binding has been created successfully and a Nexus service restarts
- **THEN** the selected authorization engine SHALL continue to return the binding's publisher allow decision

#### Scenario: Binding revocation removes final access

- **WHEN** a platform admin revokes a principal's final applicable publisher binding for domain `orders`
- **THEN** subsequent current-state decisions SHALL deny that principal publisher actions for `orders`
- **AND THEN** publisher handlers SHALL perform no schema or EventBridge side effect after the denial

#### Scenario: Overlapping binding preserves access

- **WHEN** a principal remains covered by another applicable publisher binding for the same domain after one binding is revoked
- **THEN** the selected authorization engine SHALL continue to allow publisher actions for that domain

#### Scenario: Binding mutation is audited

- **WHEN** a platform admin creates or revokes a publisher binding
- **THEN** Nexus SHALL retain an audit record identifying the binding, action, domain, typed principal, trusted actor, request identifier, timestamp, and outcome
- **AND THEN** the record SHALL NOT contain bearer tokens or sensitive token claims

### Requirement: Engine-neutral publisher decisions

Publisher schema and event handlers SHALL submit the normalized principal, closed publisher action, and target domain resource to one production authorization interface backed by the selected engine. Cognito team-group bindings SHALL match only trusted issuer-qualified group membership, and AWS-account bindings SHALL match only the trusted account in an IAM principal. The engine SHALL return default denial when no applicable binding exists or when principal context is missing, malformed, or untrusted.

#### Scenario: OAuth group and IAM account parity

- **WHEN** a trusted Cognito team group and an AWS account each have a publisher binding for `orders`
- **THEN** equivalent OAuth and IAM publisher requests for `orders` SHALL receive the same allow decision

#### Scenario: Principal type does not cross-match

- **WHEN** an OAuth principal presents a subject or group text equal to a bound AWS account identifier
- **THEN** that text SHALL NOT match the AWS-account binding
- **AND THEN** principal type and provenance SHALL remain part of the decision

#### Scenario: Authorization failure denies by default

- **WHEN** the selected authorization engine is unavailable or returns an invalid response
- **THEN** the publisher operation SHALL be denied
- **AND THEN** Nexus SHALL perform no target-domain lookup or publisher side effect
