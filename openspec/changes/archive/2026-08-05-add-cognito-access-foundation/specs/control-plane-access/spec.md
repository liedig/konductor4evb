## ADDED Requirements

### Requirement: Dedicated Cognito identity foundation

Nexus SHALL provision a dedicated Amazon Cognito user pool and OAuth/OIDC application configuration for interactive control-plane users. The supported identity contract SHALL use standard Cognito access-token claims, including the immutable subject claim and `cognito:groups`. The user pool SHALL provide a federation seam through Cognito without requiring Nexus to interpret enterprise-specific attributes.

#### Scenario: Interactive user obtains an access token

- **WHEN** an interactive user completes a configured Cognito OAuth flow
- **THEN** Cognito SHALL issue an access token for the Nexus OAuth client and control-plane scopes
- **AND THEN** Nexus SHALL identify the user by trusted standard token claims rather than caller-supplied identity fields

#### Scenario: Federated identity reaches Nexus

- **WHEN** a customer later configures an external identity provider through the Cognito federation seam
- **THEN** Nexus SHALL continue to consume the resulting standard Cognito identity and `cognito:groups` claims
- **AND THEN** Nexus SHALL NOT require an enterprise attribute-mapping scheme in its authorization contract

### Requirement: Separate OAuth and IAM API surfaces

Nexus SHALL expose separate REST API surfaces for OAuth-authenticated interactive users and AWS_IAM-authenticated callers. The IAM surface SHALL use the authenticated AWS account as the authorization and resource-binding identity. An IAM principal ARN MAY be retained as audit provenance but SHALL NOT change account-level authorization identity. A route SHALL NOT accept both mechanisms as interchangeable credentials. AWS-native event publishing, cross-account provisioning, and EventBridge delivery SHALL remain IAM-based and outside the OAuth control-plane identity mechanism.

#### Scenario: Interactive caller uses the OAuth surface

- **WHEN** an interactive client invokes an OAuth control-plane route
- **THEN** the route SHALL require a valid Cognito access token through the Nexus REQUEST authorizer
- **AND THEN** IAM request signing SHALL NOT substitute for the OAuth credential on that route

#### Scenario: AWS caller uses the IAM surface

- **WHEN** an AWS automation client invokes an IAM control-plane route
- **THEN** API Gateway SHALL authenticate the request with AWS_IAM
- **AND THEN** the trusted principal identifier used for authorization and resource binding SHALL be the caller's AWS account
- **AND THEN** the caller's IAM principal ARN MAY be retained only as audit provenance

#### Scenario: AWS-native data-plane integration operates

- **WHEN** a producer publishes through AWS-native EventBridge paths or Nexus provisions or delivers resources across accounts
- **THEN** those interactions SHALL continue to use IAM roles and resource policies
- **AND THEN** they SHALL NOT depend on Cognito user identity

### Requirement: Cognito REQUEST authorizer

The OAuth REST API SHALL use a REQUEST Lambda authorizer, not an API Gateway native Cognito user-pool authorizer. The authorizer SHALL validate the access token signature against the configured Cognito JSON Web Key Set and validate issuer, expiration, token use, and OAuth client/audience constraints before allowing invocation. It SHALL map only controlled `cognito:groups` values to coarse Nexus global roles, preserve the remaining validated Cognito group values as trusted team-group identifiers, return an execute-api policy for the requested route, and attach compact trusted principal context for downstream integrations. The context SHALL NOT contain resource entitlements.

#### Scenario: Valid access token is authorized

- **WHEN** the authorizer receives a correctly signed, unexpired Cognito access token issued for the configured user pool and OAuth client
- **THEN** it SHALL return an execute-api policy appropriate to the requested route
- **AND THEN** it SHALL attach trusted principal context containing the Cognito subject, mapped global roles, and trusted team-group identifiers derived only from validated claims

#### Scenario: Invalid token is denied

- **WHEN** a token has an invalid signature, issuer, expiration, token use, or OAuth client/audience
- **THEN** the authorizer SHALL deny the invocation
- **AND THEN** no caller-supplied identity or role value SHALL be forwarded as trusted context

#### Scenario: Authorizer stays outside resource authorization

- **WHEN** the authorizer evaluates an OAuth request
- **THEN** it SHALL NOT inspect the request body to authorize a domain or other resource
- **AND THEN** it SHALL NOT load or return the caller's complete resource entitlements
- **AND THEN** trusted team-group identifiers SHALL remain identity attributes rather than precomputed entitlements
- **AND THEN** resource-level authorization SHALL remain a downstream responsibility

### Requirement: Controlled global roles

Nexus SHALL recognize exactly two coarse global roles at this foundation stage: `platform-admin` and `platform-auditor`. These roles SHALL be represented by controlled Cognito groups and mapped by the authorizer from validated `cognito:groups` claims. `platform-admin` SHALL permit platform administrative actions, including registration decisions. `platform-auditor` SHALL permit platform-wide read access but SHALL NOT permit mutations. Publisher and subscriber access SHALL NOT be represented as global roles.

#### Scenario: Platform administrator invokes an administrative route

- **WHEN** a validated access token includes the controlled `platform-admin` group
- **THEN** the authorizer SHALL include `platform-admin` in trusted global roles
- **AND THEN** the OAuth surface SHALL permit routes designated for platform administration

#### Scenario: Platform auditor attempts a mutation

- **WHEN** a validated access token includes only the controlled `platform-auditor` group
- **THEN** the OAuth surface SHALL permit platform-wide read routes
- **AND THEN** it SHALL deny mutation and approval routes

#### Scenario: Unknown group is presented

- **WHEN** a validated access token contains a Cognito group other than a controlled global-role group
- **THEN** the authorizer SHALL NOT map that group to a Nexus global role
- **AND THEN** it SHALL preserve the group value as a trusted team-group identifier in the principal context
- **AND THEN** it SHALL NOT infer publisher, subscriber, or other resource access from that identifier

### Requirement: Trusted principal normalization

Both API surfaces SHALL normalize authenticated identity into a trusted principal context with a stable mechanism discriminator, principal identifier, and mechanism-specific provenance. OAuth principals SHALL use the Cognito subject as `principalId` and include mapped global roles and trusted Cognito team-group identifiers. IAM principals SHALL use the AWS account identifier as `principalId` and authorization identity; an IAM principal ARN MAY be included only as audit provenance. Downstream components SHALL consume this trusted context rather than request-body identity assertions. The context SHALL remain compact and SHALL NOT include resource entitlements.

#### Scenario: OAuth principal is normalized

- **WHEN** the OAuth authorizer allows a request
- **THEN** downstream integrations SHALL receive a trusted principal identifying the OAuth mechanism, Cognito subject, mapped global roles, and trusted team-group identifiers
- **AND THEN** the context SHALL be derived only from validated access-token claims

#### Scenario: IAM principal is normalized

- **WHEN** API Gateway authenticates a request on the IAM surface
- **THEN** downstream integrations SHALL receive a trusted principal whose `principalId` and resource-binding identity are the authenticated AWS account
- **AND THEN** an IAM principal ARN, when retained, SHALL be audit provenance only and SHALL NOT create role- or user-level authorization granularity

### Requirement: Bounded authorizer caching and revocation behavior

OAuth authorizer result caching SHALL be disabled by default. If enabled operationally, the cache key SHALL include the bearer token and the cache duration SHALL never exceed the token's remaining validity. Nexus SHALL use a bounded access-token lifetime because offline JWT validation cannot guarantee immediate enforcement of Cognito token revocation or user/group changes. Revocation, disablement, or group changes SHALL take effect for Nexus no later than expiration of the affected access token, and newly issued tokens SHALL be evaluated without stale policy reuse from a different token.

#### Scenario: Cached policy cannot outlive token

- **WHEN** authorizer caching is enabled for a token
- **THEN** the cached result SHALL expire no later than that token's expiration
- **AND THEN** the cache entry SHALL NOT be reused for a different bearer token

#### Scenario: Group membership changes

- **WHEN** an operator changes a user's Cognito group membership
- **THEN** newly issued access tokens SHALL be evaluated using the changed global-role and team-group identifiers
- **AND THEN** an already-issued access token SHALL cease to authorize Nexus requests no later than its configured expiration

#### Scenario: Immediate policy freshness is required

- **WHEN** an installation requires every request to receive a fresh authorizer evaluation
- **THEN** it SHALL be able to keep authorizer result caching disabled
