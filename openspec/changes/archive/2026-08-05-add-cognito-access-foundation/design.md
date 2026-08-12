## Context

The current control-plane API is an API Gateway REST API whose routes are primarily protected by `AWS_IAM`. Several write routes use direct API Gateway-to-SQS service integrations, so inserting an authentication proxy Lambda would weaken the established durable-acceptance path. Approval currently treats a Step Functions task token and caller-supplied decider field as part of the public interaction, which does not establish who is making the decision.

Konductor needs an interactive OAuth/OIDC identity path while retaining AWS account identity for automation. Resource-level policy evaluation uses the DynamoDB-backed grant engine validated by the separate authorization-engine spike. This change therefore establishes identity validation, coarse global roles, trusted Cognito team-group propagation, account-granular IAM identity, and authorization boundaries without implementing the resource policy engine.

## Goals / Non-Goals

**Goals:**

- Add a dedicated Cognito user pool and OAuth client foundation for interactive control-plane users.
- Keep OAuth and IAM API surfaces separate while normalizing both identities into a trusted principal contract.
- Validate Cognito access tokens in a REST API REQUEST Lambda authorizer, map controlled groups to coarse global roles, and preserve other validated group values as trusted team-group identifiers.
- Preserve direct API Gateway-to-SQS integrations and carry authenticated principal context into asynchronous consumers.
- Require OAuth `platform-admin` authorization for approval independently of Step Functions callback state.
- Bound stale authorizer decisions and token-revocation exposure.
- Leave a clean Cognito federation seam without coupling Konductor to enterprise-specific attributes.

**Non-Goals:**

- Enterprise identity-provider attribute mapping, customer-specific group mapping, or federation setup guidance.
- Publisher API design, subscriber entitlements/catalog design, resource bindings, or schema promotion.
- Resource-body authorization in the authorizer or materializing all entitlements in authorizer context.
- Replacing IAM for AWS-native publishing, cross-account provisioning, or EventBridge delivery.
- Implementing the DynamoDB resource-policy engine validated by `evaluate-authorization-engine`.

## Decisions

### 1. Deploy separate REST API surfaces

The control plane will have distinct OAuth and IAM REST API surfaces, each at a static `v1` stage. A route is intentionally exposed on one surface according to its caller contract rather than accepting OAuth and SigV4 as equivalent credentials on one method. Approval is OAuth-only. AWS automation and AWS-native paths remain IAM-based.

This separation avoids ambiguous principal semantics, permits independent API Gateway authorization configuration, and prevents later policy code from accidentally treating an interactive subject as an AWS account principal. Shared handlers may still be used where behavior is identical, provided they consume the normalized principal contract.

### 2. Use a dedicated Cognito user pool as the interactive identity broker

The stack will provision a dedicated user pool, an OAuth application client suitable for the interactive application, controlled `platform-admin` and `platform-auditor` groups, and configuration outputs needed by the OAuth client and authorizer. Cognito is the federation seam: a future customer may connect an external IdP and manage Cognito group membership directly or add pre-token customization. Konductor consumes only standard Cognito claims and `cognito:groups`; controlled group names map to global roles, while other validated values are carried as opaque trusted team-group identifiers. Konductor does not interpret arbitrary enterprise attributes or define resource bindings in this change.

OAuth configuration must use access tokens for API access. ID tokens are not accepted by the authorizer. Redirect URIs, logout URIs, and supported OAuth flows are deployment parameters with secure defaults appropriate to the chosen interactive client; client secrets are not embedded in browser-delivered code.

### 3. Replace native Cognito authorization with a REQUEST Lambda authorizer

The OAuth API will use a REQUEST authorizer with the `Authorization` header as its identity source. The authorizer validates:

- JWT signature using keys from the configured user-pool JWKS;
- expected issuer and unexpired time bounds;
- `token_use` equal to `access`;
- expected OAuth client/audience semantics for the configured app client.

After token validation, it maps only exact controlled group names to `platform-admin` and `platform-auditor`. Other validated group values have no global-role meaning but are preserved as opaque trusted team-group identifiers for later resource-binding evaluation. The authorizer returns an execute-api policy scoped to the invoked route and a compact trusted context containing subject, global roles, and team groups. It does not inspect request bodies, query a resource entitlement store, return an entitlement set, or interpret team groups as permissions.

JWKS documents may be cached by key identifier with bounded refresh and retry when a key is unknown, so Cognito key rotation does not require redeployment. Token values and sensitive claims must not be logged.

### 4. Normalize identities without erasing their provenance

Downstream code receives a common logical contract:

```text
TrustedPrincipal {
  authenticationMechanism: "oauth" | "iam"
  principalId: string
  oauth?: {
    subject: string
    issuer: string
    clientId: string
    globalRoles: ["platform-admin" | "platform-auditor"]
    teamGroups: string[]
  }
  iam?: {
    accountId: string
    principalArn?: string
  }
}
```

The exact serialized representation may use compact scalar authorizer-context fields and a versioned queue envelope to satisfy API Gateway constraints. For OAuth, `principalId` is the Cognito `sub`; `teamGroups` carries validated non-global-role group identifiers without entitlements. For IAM, both `principalId` and resource-binding identity are the authenticated AWS account ID. The optional `principalArn` field is audit provenance and must never introduce role- or user-level authorization granularity. The mechanism discriminator and required mechanism-specific fields are mandatory.

API proxy Lambdas parse subject, global roles, and team groups from trusted OAuth authorizer context, or account identity and optional ARN provenance from IAM request context. Direct SQS mapping templates serialize the same fields into the versioned queue envelope. Request-body identity, account, group, role, and `decidedBy` values are never allowed to overwrite them. Team groups remain compact identity attributes and are never expanded into entitlement records in authorizer or queue context.

### 5. Keep coarse route access separate from resource authorization

The authorizer enforces authentication and global route gates:

- `platform-admin` can access platform administration and approval routes.
- `platform-auditor` can access platform-wide read routes but cannot mutate or approve.
- Publisher and subscriber permissions are not global roles and are not inferred from Cognito team groups in this change; team-group identifiers are only retained as trusted inputs for later binding evaluation.

Handlers and queue consumers remain the resource-authorization enforcement points. For asynchronous commands, a successful SQS service integration returns `202` once the message is durable. The consumer must authorize the principal against the target resource before mutation and report deterministic denial through the command outcome mechanism. The policy evaluator behind that step is supplied by the authorization-engine change; the `TrustedPrincipal` input and allow/deny enforcement boundary are stable here.

### 6. Disable authorizer result caching by default

Authorizer result caching defaults to zero seconds. If operators enable it, the bearer token is part of the cache key and the effective cache duration cannot exceed the token's remaining lifetime.

Cognito access tokens are locally validated JWTs, so revoking a refresh token, disabling a user, or changing groups does not guarantee immediate invalidation of an already-issued access token. The user pool therefore uses a bounded, configurable access-token lifetime. The explicit security guarantee is that stale access ends no later than access-token expiration; newly issued tokens receive fresh group mapping and cannot reuse another token's cached result. Deployments requiring per-request authorizer evaluation keep result caching disabled.

### 7. Make approval identity independent from callback state

`POST /domains/registrations/decide` exists only on the OAuth API and requires `platform-admin`. API Gateway supplies the trusted OAuth principal to the decide Lambda. The request no longer needs an authoritative `decidedBy`; if sent during migration, it is ignored or rejected according to the final OpenAPI schema. The Lambda uses the Cognito subject, and a safe display claim if needed, to produce the workflow's `decidedBy`.

The Step Functions task token remains necessary for `SendTaskSuccess` or `SendTaskFailure`, but possession of the token never bypasses OAuth. Approval email no longer contains the token or an unauthenticated bearer decision URL; it directs the operator to sign in to the OAuth experience. The workflow's state transitions and event-emission responsibilities remain unchanged.

## Risks / Trade-offs

- **Two API surfaces increase infrastructure and contract management.** Keep common schemas, handlers, and trusted-principal types shared while testing that route exposure and authorization remain intentionally distinct.
- **Offline JWT validation has bounded, not immediate, revocation.** Default to no authorizer-result cache, configure a short bounded access-token lifetime, and state the expiration-bound guarantee explicitly.
- **Authorizer context is size- and type-constrained.** Keep only subject, coarse global roles, opaque team-group identifiers, and required provenance in context; never include resource entitlements. Use a compact representation and a versioned compact queue envelope, and fail closed rather than silently changing identity if validated group context cannot be represented safely.
- **VTL mapping errors could permit principal spoofing or context loss.** Generate principal fields exclusively from API Gateway context and test OAuth and IAM mappings with malicious identity fields in request bodies.
- **JWKS rotation or Cognito outages can deny interactive access.** Cache validated keys, refresh on unknown key identifiers, fail closed, and expose safe operational metrics without logging tokens.
- **Removing bearer approval breaks existing email-driven operations.** Deploy the Cognito/OAuth surface and authenticated approval experience before removing the old route, communicate the cutover, and ensure pending approvals remain reachable through authenticated platform-admin flows.
- **Resource authorization cannot be completed by this change alone.** Queue consumers must fail closed at the defined enforcement boundary until the selected authorization-engine implementation is integrated.

## Migration Plan

1. Provision the Cognito user pool, OAuth client/configuration, controlled groups, and authorizer without changing IAM routes.
2. Add the OAuth REST API and trusted-principal model; validate token, global-role, team-group, account-identity, and context behavior.
3. Add subject/global-role/team-group propagation to proxy integrations and direct SQS mapping templates while preserving existing queue integrations and response semantics; keep IAM `principalId` account-granular.
4. Add consumer-side authorization interfaces that accept `TrustedPrincipal`; integrate the DynamoDB policy evaluator validated by the authorization-engine spike before enabling protected OAuth resource mutations.
5. Move approval to the OAuth-only route, derive `decidedBy` from trusted context, and update notifications to require sign-in without exposing task tokens as bearer credentials.
6. Remove the old approval authorization path and caller-asserted decider contract after verifying pending workflows can still be completed through the OAuth route.
7. Retain IAM routes and AWS-native IAM/resource-policy behavior throughout the migration.

Rollback before step 6 can disable the OAuth routes while leaving IAM behavior intact. After the breaking approval cutover, rollback must not re-enable unauthenticated task-token authorization; operators should pause decisions or restore a previously authenticated approval deployment instead.

## Open Questions

- How will the DynamoDB grant engine and storage model implement the downstream `TrustedPrincipal` authorization interface? This is validated by `evaluate-authorization-engine` and does not change the identity contract defined here.
- What customer-facing guidance and supported templates should be provided for external IdP mappings or Cognito pre-token customization? That guidance is deferred; the supported Konductor input remains standard Cognito identity plus trusted `cognito:groups`, with non-global values carried opaquely as team-group identifiers.
