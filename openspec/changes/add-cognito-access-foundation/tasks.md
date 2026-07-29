## 1. Principal Contracts

- [x] 1.1 Add a versioned `TrustedPrincipal` model whose OAuth variant carries subject, controlled global roles, and trusted team-group identifiers and whose IAM variant uses the AWS account as `principalId` with optional principal ARN audit provenance
- [x] 1.2 Add serialization and validation tests proving malformed or mixed-mechanism principal contexts fail closed, OAuth team groups survive round trips, and IAM principal ARN cannot alter account-level authorization identity
- [x] 1.3 Define the downstream resource-authorization interface that accepts `TrustedPrincipal` without implementing the DynamoDB grant engine
- [x] 1.4 Keep OAuth resource mutations disabled or fail closed until the authorization-engine change supplies the resource-policy evaluator

## 2. Cognito Identity Infrastructure

- [x] 2.1 Add a dedicated Cognito user pool and configurable access-token lifetime to the SAM control-plane stack
- [x] 2.2 Add the OAuth app client, supported OAuth scopes and flows, and parameterized callback/logout URLs without exposing a browser client secret
- [x] 2.3 Add controlled Cognito groups for `platform-admin` and `platform-auditor`
- [x] 2.4 Export the user-pool issuer, app-client identifier, and interactive OAuth configuration needed by the authorizer and UI
- [x] 2.5 Add template tests validating Cognito resources, group names, token lifetime, and secure client settings

## 3. OAuth REQUEST Authorizer

- [x] 3.1 Add the REQUEST Lambda authorizer binary and its least-privilege SAM resources for the OAuth REST API
- [x] 3.2 Implement Cognito access-token validation for signature, issuer, expiration, `token_use`, and configured client/audience
- [x] 3.3 Implement bounded JWKS caching with refresh on an unknown key identifier and fail-closed error handling
- [x] 3.4 Map exact controlled `cognito:groups` values to global roles and preserve other validated values as opaque trusted team-group identifiers without assigning permissions
- [x] 3.5 Generate route-scoped execute-api allow/deny policies plus compact subject, global-role, and team-group authorizer context without inspecting request bodies or loading resource entitlements
- [x] 3.6 Disable authorizer-result caching by default and enforce token-specific cache keys and token-expiration bounds if caching is configured
- [x] 3.7 Add authorizer tests covering valid tokens, every validation failure, global-role mapping, unknown team-group preservation, compact context, route gates, JWKS rotation, caching bounds, and token redaction

## 4. Separate API Surfaces

- [x] 4.1 Split the OpenAPI contract and SAM resources into explicit OAuth and AWS_IAM REST API surfaces at the static `v1` stage
- [x] 4.2 Attach the REQUEST authorizer only to OAuth methods and retain AWS_IAM authorization with AWS account `principalId` on IAM methods
- [x] 4.3 Assign current control-plane routes to the appropriate surface, with approval exposed only on OAuth and no mixed-auth method
- [x] 4.4 Preserve IAM-based AWS-native publishing, cross-account provisioning, and EventBridge delivery configuration unchanged
- [x] 4.5 Add contract and template tests that reject unintended unauthenticated, mixed-auth, or IAM approval exposure

## 5. Trusted Context Propagation

- [x] 5.1 Parse OAuth subject, global roles, and trusted team groups from authorizer context and IAM account identity plus optional ARN audit provenance from request context into the common trusted-principal type in proxy Lambda handlers
- [x] 5.2 Update domain direct API Gateway-to-SQS mapping templates to serialize OAuth subject/global-role/team-group context or IAM account identity into a versioned trusted-principal envelope while preserving existing service integrations
- [x] 5.3 Ensure mapping templates overwrite or reject caller-supplied identity, account, group, role, and decider fields
- [x] 5.4 Update domain queue consumers to parse the trusted-principal envelope and invoke the resource-authorization interface before state changes
- [x] 5.5 Preserve `202 Accepted` as durable queue acceptance and emit the established deterministic failure outcome when consumer-side authorization denies a command
- [x] 5.6 Add mapping and consumer tests for OAuth team-group preservation, absence of entitlements, IAM account-level binding despite differing principal ARNs, malicious body identity fields, missing context, and authorization denial without mutation

## 6. OAuth-Only Approval

- [x] 6.1 Change the approval OpenAPI schema and route to require OAuth `platform-admin` and stop treating caller-supplied `decidedBy` as authoritative
- [x] 6.2 Update the decide Lambda to derive `decidedBy` from trusted OAuth principal context before issuing Step Functions callbacks
- [x] 6.3 Reject task-token-only, `platform-auditor`, unauthenticated, and IAM approval attempts before any Step Functions callback
- [x] 6.4 Update approval email generation to direct operators to sign in and remove task tokens and unauthenticated decision URLs from email content
- [x] 6.5 Verify existing pending approval workflows can complete through the authenticated route while task tokens remain callback state
- [x] 6.6 Add approval tests covering accept, reject, trusted decider identity, token-only denial, role denial, expired callback state, and notification content

## 7. Migration, Operations, and Documentation

- [x] 7.1 Add metrics and safe logs for authorizer allows/denies, token-validation categories, JWKS refresh, and consumer authorization denials without logging tokens or sensitive claims
- [x] 7.2 Add deployment checks and a cutover sequence that provisions OAuth access before removing the bearer approval path and never re-enables unauthenticated approval during rollback
- [x] 7.3 Update `docs/development/architecture.md` with separate API surfaces, Cognito identity flow, trusted team-group propagation, account-granular IAM identity, and asynchronous authorization boundaries
- [x] 7.4 Update `docs/development/decisions.md` with the OAuth/IAM separation, REQUEST authorizer, global-role and team-group handling, account-level IAM binding, caching/revocation, and approval decisions
- [x] 7.5 Document the dependency on `evaluate-authorization-engine` and the deferred enterprise federation/group-customization guidance without defining publisher or subscriber bindings
- [x] 7.6 Run formatting, unit and integration tests, linting, SAM/OpenAPI validation, and strict OpenSpec validation
