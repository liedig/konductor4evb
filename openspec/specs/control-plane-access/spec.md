# Capability: Control Plane Access

## Purpose

How callers authenticate to the Konductor control plane and how that identity becomes a trusted principal: a dedicated Cognito user pool for interactive users, separate OAuth and AWS_IAM API surfaces, a REQUEST Lambda authorizer that validates tokens and maps controlled global roles, trusted principal normalization for downstream components, and bounded authorizer caching and revocation behavior.

## Requirements

### Requirement: Dedicated Cognito identity foundation

Konductor SHALL provision a dedicated Amazon Cognito user pool and OAuth/OIDC application configuration for interactive control-plane users. The supported identity contract SHALL use standard Cognito access-token claims, including the immutable subject claim and `cognito:groups`. The user pool SHALL provide a federation seam through Cognito without requiring Konductor to interpret enterprise-specific attributes.

#### Scenario: Interactive user obtains an access token

- **WHEN** an interactive user completes a configured Cognito OAuth flow
- **THEN** Cognito SHALL issue an access token for the Konductor OAuth client and control-plane scopes
- **AND THEN** Konductor SHALL identify the user by trusted standard token claims rather than caller-supplied identity fields

#### Scenario: Federated identity reaches Konductor

- **WHEN** a customer later configures an external identity provider through the Cognito federation seam
- **THEN** Konductor SHALL continue to consume the resulting standard Cognito identity and `cognito:groups` claims
- **AND THEN** Konductor SHALL NOT require an enterprise attribute-mapping scheme in its authorization contract

### Requirement: Separate OAuth and IAM API surfaces

Konductor SHALL expose separate REST API surfaces for OAuth-authenticated interactive users and AWS_IAM-authenticated callers. The IAM surface SHALL use the authenticated AWS account as its resource-authorization identity and MAY retain the IAM principal ARN only as audit provenance. A route exposed on both surfaces SHALL enforce the same operation contract and authorization semantics after trusted-principal normalization. AWS-native direct EventBridge publishing, cross-account provisioning, and EventBridge delivery SHALL remain IAM-based and outside the OAuth control-plane identity mechanism.

API Gateway stages SHALL be named after platform environments (`dev`, `staging`, `prod`) instead of a static version stage. The stage name SHALL be available as `$context.stage` in mapping templates and request contexts. Publish and schema write operations SHALL use the stage to infer the target environment. Environment-agnostic operations (register, approve, list, describe) SHALL behave identically on any stage.

Binding administration SHALL be exposed on the OAuth surface only, because it is an interactive platform-administration function. The OAuth authorizer's resolution of the `platform-admin` global role from controlled Cognito groups SHALL remain independent of the grant engine's binding storage so that binding administration remains available when the bindings table is empty.

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

#### Scenario: Binding administration requires OAuth platform-admin

- **WHEN** a caller invokes a binding administration route
- **THEN** the route SHALL exist only on the OAuth surface
- **AND THEN** the caller SHALL hold the `platform-admin` global role resolved by the OAuth authorizer
- **AND THEN** the grant engine's binding storage SHALL NOT be consulted to establish the `platform-admin` role

#### Scenario: AWS-native data-plane integration operates

- **WHEN** a producer publishes directly through AWS-native EventBridge paths or Konductor provisions or delivers resources across accounts
- **THEN** those interactions SHALL continue to use IAM roles and resource policies

### Requirement: Cognito REQUEST authorizer

The OAuth REST API SHALL use a REQUEST Lambda authorizer, not an API Gateway native Cognito user-pool authorizer. The authorizer SHALL validate the access token signature against the configured Cognito JSON Web Key Set and validate issuer, expiration, token use, and OAuth client/audience constraints before allowing invocation. It SHALL map only controlled `cognito:groups` values to coarse Konductor global roles, preserve the remaining validated Cognito group values as trusted team-group identifiers, return an execute-api policy for the requested route, and attach compact trusted principal context for downstream integrations. The context SHALL NOT contain resource entitlements.

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

Konductor SHALL recognize exactly two coarse global roles at this foundation stage: `platform-admin` and `platform-auditor`. These roles SHALL be represented by controlled Cognito groups and mapped by the authorizer from validated `cognito:groups` claims. `platform-admin` SHALL permit platform administrative actions, including registration decisions. `platform-auditor` SHALL permit platform-wide read access but SHALL NOT permit mutations. Publisher and subscriber access SHALL NOT be represented as global roles.

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
- **THEN** the authorizer SHALL NOT map that group to a Konductor global role
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

OAuth authorizer result caching SHALL be enabled with a 300-second TTL, with the cache key composed of the bearer token, HTTP method, and resource path. The route-scoped key is required for correctness: the authorizer returns a policy scoped to the invoked route, so a token-only key would apply one route's policy to a caller's other routes. (Zero-TTL caching cannot be expressed through the SAM authorizer configuration — a declared `0` is dropped by the transform — so a short route-scoped TTL is the honest default.) The cache duration SHALL never exceed the token's remaining validity. Konductor SHALL use a bounded access-token lifetime because offline JWT validation cannot guarantee immediate enforcement of Cognito token revocation or user/group changes. Revocation, disablement, or group changes SHALL take effect for Konductor no later than expiration of the affected access token, and newly issued tokens SHALL be evaluated without stale policy reuse from a different token.

#### Scenario: Cached policy cannot outlive token

- **WHEN** authorizer caching is enabled for a token
- **THEN** the cached result SHALL expire no later than that token's expiration
- **AND THEN** the cache entry SHALL NOT be reused for a different bearer token

#### Scenario: Cached policy is scoped to the route

- **WHEN** a caller's authorizer result for one route is cached
- **THEN** a request to a different route or method with the same token SHALL trigger a fresh authorizer evaluation
- **AND THEN** a cached Allow for one route SHALL NOT authorize any other route

#### Scenario: Group membership changes

- **WHEN** an operator changes a user's Cognito group membership
- **THEN** newly issued access tokens SHALL be evaluated using the changed global-role and team-group identifiers
- **AND THEN** an already-issued access token SHALL cease to authorize Konductor requests no later than its configured expiration

### Requirement: Entitlement administration is OAuth-surface only

Entitlement administration routes (`POST /entitlements`, `GET /entitlements`, `DELETE /entitlements/{entitlementId}`) SHALL exist only on the OAuth control-plane surface, wired to the Konductor REQUEST authorizer. The IAM surface SHALL NOT expose entitlement administration in this capability version. Administration authorization SHALL combine the `platform-admin` global role, resolved by the OAuth authorizer from controlled Cognito groups independently of grant storage, with resource-role bindings evaluated by the grant engine: a `publisher` binding on the entitlement's producer domain authorizes create and revoke for that producer domain, and a `subscriber` binding authorizes listing the bound consumer domain's entitlements. Subscription resource routes and the consumer schema catalog SHALL remain available on both surfaces with identical post-normalization authorization semantics.

#### Scenario: Entitlement routes reject IAM-surface access

- **WHEN** any caller requests an entitlement administration route on the IAM control-plane surface
- **THEN** no such route SHALL exist

#### Scenario: Producer self-service uses normalized bindings

- **WHEN** an OAuth caller with a `publisher` binding on producer domain `orders` administers entitlements covering `orders`
- **THEN** the OAuth surface SHALL authorize the operation through the grant engine without requiring the `platform-admin` role

#### Scenario: Subscription resource routes remain dual-surface

- **WHEN** a caller invokes subscription create, state, delete, list, or detail routes, or the consumer schema catalog
- **THEN** those routes SHALL exist on both the OAuth and IAM surfaces
- **AND THEN** both SHALL enforce the same engine decision after trusted-principal normalization

### Requirement: Direct SRP authentication for custom login clients

The Cognito user-pool client SHALL allow `ALLOW_USER_SRP_AUTH` and `ALLOW_REFRESH_TOKEN_AUTH` in its explicit auth flows, so first-party interactive clients (e.g. the Konductor web UI) can authenticate users with a custom login experience without the hosted UI. Tokens obtained through SRP SHALL be indistinguishable to the OAuth authorizer from tokens obtained via the hosted UI: the same claims, group mappings, and trusted-principal normalization SHALL apply. Self-service sign-up SHALL remain disabled; users remain admin-provisioned.

#### Scenario: Custom login obtains usable tokens

- **WHEN** a user signs in through a first-party client using SRP against the user pool
- **THEN** Cognito SHALL issue access, ID, and refresh tokens carrying the user's group memberships
- **AND THEN** the OAuth control-plane API SHALL accept the access token and resolve the same trusted principal as a hosted-UI login

#### Scenario: Refresh without re-authentication

- **WHEN** a first-party client presents a valid refresh token
- **THEN** Cognito SHALL issue a new access token within the configured validity bounds
- **AND THEN** the user's group memberships SHALL be re-evaluated from current pool state

#### Scenario: No self-registration

- **WHEN** an unprovisioned identity attempts to sign up through any enabled flow
- **THEN** Cognito SHALL reject the attempt
- **AND THEN** only admin-created users SHALL be able to authenticate

### Requirement: Platform access administration stays customer-controlled

Global-role group membership SHALL be administered by the customer operator through Cognito administrative actions. Konductor SHALL NOT provide API routes, console features, setup tooling, or configuration settings that create users or modify membership of the controlled global-role groups. No Konductor-authorized path SHALL allow a caller to grant platform global roles to itself or to others; resource-role bindings and entitlements SHALL NOT expand into global roles. The OAuth authorizer's resolution of `platform-admin` SHALL remain independent of grant storage, so an operator with Cognito administrative access can always establish or recover the first platform administrator.

#### Scenario: No self-escalation path exists

- **WHEN** an authenticated caller without a platform global role attempts to obtain one through any Konductor API, binding, entitlement, or configuration mechanism
- **THEN** no such mechanism SHALL exist
- **AND THEN** the attempt SHALL fail without modifying any group membership or global-role state

#### Scenario: Operator establishes the first platform administrator

- **WHEN** an operator with Cognito administrative access adds a user to the controlled `platform-admin` group on a fresh installation
- **THEN** that user's next issued token SHALL resolve the `platform-admin` global role
- **AND THEN** this SHALL NOT depend on any existing Konductor principal, binding, or entitlement
