# Cognito/OAuth interactive identity, dual API surfaces, trusted-principal propagation, OAuth-only approval—without the DynamoDB grant engine

[Documentation home](../README.md)

- Recorded date: 2026-07-29
- Change ID: `add-cognito-access-foundation`
- Area: Authorization
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Dual API surfaces**: split the single IAM-auth REST API into a separate OAuth surface (`openapi-oauth.yaml`, Cognito + REQUEST authorizer) and an IAM surface (`openapi-iam.yaml`, SigV4), each at the static `v1` stage. No mixed-auth method; approval is OAuth-only.

**Cognito identity**: dedicated user pool with configurable access-token lifetime, OAuth app client (no browser client secret exposed), controlled groups `platform-admin` and `platform-auditor`.

**REQUEST authorizer**: Lambda authorizer validates Cognito access tokens (signature via JWKS, issuer, expiration, `token_use=access`, client/audience), maps `cognito:groups` to `GlobalRole` enum values, preserves other groups as opaque team-group identifiers, and emits route-scoped allow/deny policies (`platform-admin` = any method, `platform-auditor` = GET only). JWKS caching with refresh on unknown `kid`; authorizer-result caching disabled by default. No token material in logs or authorizer context.

**TrustedPrincipal contract**: versioned, tag-based enum—OAuth variant carries subject, issuer, client ID, global roles, team groups; IAM variant carries AWS account ID as `principalId` with optional principal ARN audit provenance. Fail-closed deserialization: missing discriminator, unknown mechanism, or mixed fields is an error, never a silently reinterpreted identity.

**Trusted context propagation**: VTL mapping templates inject a versioned envelope into SQS message attributes (sourced from `$context.authorizer.*` / `$context.identity.*`, never the request body); consumer handlers reconstruct `TrustedPrincipal` from the envelope and invoke `ResourceAuthorizer` before state changes; proxy read handlers parse the principal from `requestContext` for audit logging. **FoundationAuthorizer** (since renamed `IamOnlyAuthorizer`): interim `ResourceAuthorizer` implementation—allows IAM mutations, denies OAuth mutations (fail-closed)—until the DynamoDB grant engine lands. Denied commands emit the established deterministic failure outcome (EventBridge `*Failed` + SQS ack, no retry).

**OAuth-only approval**: `decidedBy` is derived from the trusted OAuth principal's `principalId`, not from caller-supplied input; the `decidedBy` field is removed from `DecideRequest` and rejected by `deny_unknown_fields`. The email-link bearer GET approval path is removed; approval emails direct operators to sign in. Task tokens remain the Step Functions callback mechanism, supplied in the authenticated POST body. The decide handler verifies OAuth mechanism and `platform-admin` role (defense-in-depth) before any Step Functions callback.

**Dependency**: this change establishes the identity and propagation substrate for the production authorization engine (spiked in `evaluate-authorization-engine`); enterprise federation and group-customization guidance are deferred to that change.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-08-05-add-cognito-access-foundation/proposal.md). The recorded date above is not necessarily the archive or merge date.

## Current relevance

The interim authorizer and static API stage are dated foundation choices. Continue with [the grant engine](add-grant-engine.md), [publisher APIs](add-publisher-apis.md), and [entitlements](add-subscriber-entitlements.md).

[Decision index](README.md)
