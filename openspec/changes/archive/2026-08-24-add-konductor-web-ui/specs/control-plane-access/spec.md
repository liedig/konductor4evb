# Delta: control-plane-access (add-konductor-web-ui)

## MODIFIED Requirements

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
