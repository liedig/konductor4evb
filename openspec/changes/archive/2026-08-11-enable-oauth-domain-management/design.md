# Design: enable-oauth-domain-management

## Context

`IamOnlyAuthorizer` currently fails closed on every OAuth mutation to domain register/update (see proposal). Bindings already support issuer-qualified Cognito team groups as principals (`BindingPrincipal::CognitoGroup`), and `TrustedPrincipal` separates global platform groups from `team_groups`. The approval workflow persists its Step Functions task token on the domain record, but no read path projects it.

## Goals / Non-Goals

**Goals:** complete the OAuth authorization story for domain management so the web UI can offer request/edit/approve journeys; keep IAM automation byte-for-byte compatible; reuse the grant engine as the single authorization model.

**Non-goals:** platform configuration management (parked — environments/emails/tags stay deploy-time parameters); schema lifecycle semantics (#11); retroactive ownership for previously approved domains (see Migration).

## Decisions

1. **Ownership lives in the grant engine as `ResourceRole::Owner`** — not as a domain-record attribute. "Who may manage this domain" is the same kind of question as "who may publish", and one engine keeps fail-closed reasoning uniform. Alternative considered: an `ownerGroups` field on the domain record (simpler, but creates a second, divergent authorization mechanism).

2. **Approval writes the owner binding transactionally.** The decide handler (ACCEPT path) writes the domain activation and the owner binding (cognito-group = the requesting team's chosen group) in one transaction, mirroring the entitlement revoke-audit pattern. The registration request records the chosen owning group on the domain item at request time so approval needs no new input.

3. **Owning team must be one of the caller's `team_groups`.** The token already excludes global groups from `team_groups`, so "platform teams cannot own domains" is enforced by construction: a caller in zero team groups cannot register. Users in several teams pick the owning team in the request.

4. **Scoped reads via ownership union.** For non-platform OAuth callers, `GET /domains` filters to domains where any caller team group holds an owner binding; `GET /domains/{name}` allows owners and platform roles, denies others. The names-only directory (for pickers) is a separate, deliberately thin projection: `domainName`, `description`, `status` — no owner account IDs or emails. Domain names are already disclosed to entitled consumers via catalog `producerDomain`, so the projection adds no meaningful exposure; the full record is what gets protected.

5. **`GET /domains/registrations/pending` as a dedicated admin endpoint** rather than projecting `taskToken` onto the general list. The token is a capability (its holder can decide); scoping it to a platform-admin route keeps the capability boundary explicit. The notification email keeps working as today.

6. **Producer catalog scope in `resolve_scope`.** `Scope::Producer(domain)` alongside Consumer/Platform: requires a publisher binding on `domain`, sources from that domain's registries unfiltered by entitlements. A consumer-domain parameter keeps its existing meaning. Alternative considered: self-entitlements written at approval (data convention instead of code — rejected: makes "self" a permanent special case in a table meant for cross-domain grants).

7. **Enable/disable is shallow.** Disabling rejects new publishes and new subscription activations for the domain; existing EventBridge infrastructure, subscriptions, and entitlements are retained; enabling reverses it. No teardown machinery. Alternative considered: provisioning-level teardown — out of scope, and destructive.

8. **SRP on the existing pool client.** Add `ALLOW_USER_SRP_AUTH` and `ALLOW_REFRESH_TOKEN_AUTH` to `ExplicitAuthFlows`; no new client, no hosted-UI removal. The custom login page exchanges credentials via Cognito SRP and stores the resulting tokens; the OAuth authorizer is unchanged because it validates pool JWTs regardless of how they were minted.

## Risks / Trade-offs

- [Owner role widens every role-keyed match/persist path] → Compiler exhaustiveness does the sweeping; add grant-engine tests for owner semantics before touching handlers (TDD).
- [Scoped `GET /domains` is BREAKING for interactive callers who relied on full visibility] → Only OAuth interactive clients are affected (IAM automation unchanged); the UI is the primary consumer and is built against the new contract; flagged in proposal.
- [Pending endpoint leaks decision capability if authorization regresses] → Platform-admin check is the first statement of the handler; contract test asserts 403 for owner/auditor/non-platform callers.
- [Domains approved before this change have no owner binding and become invisible/unmanageable to their teams] → Migration: one-off backfill (script or CLI target) writing owner bindings from a provided domain→team-group mapping; document as an operator step. Alternative: leave to admins to grant manually via the bindings API.

## Migration Plan

1. Deploy stack + Lambdas (new role, endpoints, auth flows are additive; the breaking read change ships in the same deploy).
2. Run the owner-binding backfill for pre-existing domains (mapping supplied by the platform team).
3. Update the oauth-authentication-guide with SRP login + seed-users instructions.
Rollback: revert the deployment; owner bindings are inert data under the old code (unknown role is ignored by old readers — verify in tests).

## Open Questions

- Exact `DomainStatus` vocabulary for disable (`DISABLED` vs `SUSPENDED`) and whether "disabled" should also pause existing subscription deliveries — semantics chosen conservatively (no pause); revisit if operations wants stronger isolation.
