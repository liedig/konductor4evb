# Proposal: enable-oauth-domain-management

## Why

Interactive OAuth users can authenticate and read, but `IamOnlyAuthorizer` deliberately blocks every domain mutation on the OAuth surface ("interactive OAuth mutations are not yet enabled"), and ownership is expressed only as account/email attributes — the grant engine has no owner role, so "my team's domains" is not representable. The web UI (add-konductor-web-ui) needs domain owners to request and manage domains and platform admins to approve them, which requires completing this authorization story first.

## What Changes

- Add `ResourceRole::Owner` to the grant engine; registration approval writes an owner binding (cognito-group → domain) for the requesting team's chosen group. Platform groups (`platform-admin`, `platform-auditor`) cannot own domains — the owning team must be one of the caller's non-global team groups.
- Replace `IamOnlyAuthorizer` on domain register/update with grant-engine authorization: any authenticated user with at least one team group may request a domain; update (description/emails/tags only) requires an owner binding or platform-admin. Environments remain platform-set; owners cannot change them.
- Scope domain reads: non-platform OAuth callers see management views only for domains their team groups own (owner binding); platform roles see all. **BREAKING**: `GET /domains` no longer returns full domain records (owner account IDs, emails) to arbitrary authenticated callers.
- Provide a names-only domain directory projection (domainName, description, status) for authenticated users, backing pickers such as the entitlement-grant form; full records stay owner/platform-scoped.
- Add `GET /domains/registrations/pending`: platform-admin endpoint returning pending registrations including the Step Functions task token, so the approvals inbox can call decide without the email.
- Add platform-only domain enable/disable lifecycle (publishing and new subscription activations rejected while disabled; existing subscriptions and infrastructure retained).
- Catalog: add a producer scope — a caller with a publisher binding on domain D may view D's own schemas and latest versions without a consumer entitlement.
- Cognito: enable `ALLOW_USER_SRP_AUTH` and `ALLOW_REFRESH_TOKEN_AUTH` on the user-pool client so a custom login page can authenticate without the hosted UI.
- Tooling: `scripts/seed-users.sh` + `make seed-users` creating minimal test personas (platform admin; domain-team member) with permanent passwords.

## Capabilities

### New Capabilities

- `domain-ownership`: Owner resource role in the grant engine, ownership binding lifecycle (written at approval), team-group ownership rules (platform groups cannot own), and the scoped-read/directory visibility model built on it.

### Modified Capabilities

- `domain-management`: OAuth callers may register and update domains under the new authorization rules; list/describe reads are scoped by ownership with a names-only directory projection; pending-registrations endpoint for approvers; platform-only enable/disable lifecycle.
- `schema-management`: catalog gains a producer scope (publisher binding ⇒ view own domain's schemas/versions without a consumer entitlement).
- `control-plane-access`: interactive clients may authenticate via SRP (custom login) in addition to the hosted UI.

## Impact

- **Grant engine** (`lib/konductor-common/src/grant.rs`): new `ResourceRole` variant; every role-keyed code path gains a third role.
- **Domain management**: register/update handlers re-authorized; approval workflow (or decide handler) writes the owner binding; list/describe scoping; new pending endpoint; enable/disable transitions.
- **Catalog** (`list-schemas`): producer scope in `resolve_scope`.
- **Stack** (`konductor-core-stack.yaml`): user-pool client auth flows; new route/integration for pending registrations.
- **API contracts** (`api/openapi-oauth.yaml`): new/changed endpoints and response projections.
- **Tooling**: new `scripts/seed-users.sh`, Makefile target.
- **Consumers**: IAM automation is unaffected (IAM paths unchanged); any existing interactive scripts that relied on unscoped `GET /domains` output must adapt to the projection split.
- **Web UI** (add-konductor-web-ui) depends on this change for all write journeys and the approvals inbox.
