# Tasks: enable-oauth-domain-management

## 1. Grant engine: owner role

- [x] 1.1 Red tests: `owner` role parses/serializes (`as_str`, serde), satisfies owner checks, and does not satisfy publisher/subscriber checks
- [x] 1.2 Add `ResourceRole::Owner` and sweep every role-keyed match (grant engine, store, bindings API validation) for exhaustiveness
- [x] 1.3 Green + `make check`; commit `feat(grants): add owner resource role`

## 2. Ownership at registration and approval

- [ ] 2.1 Red tests: OAuth register requires `owningTeam` ∈ caller team groups; platform groups and absent teams denied; IAM callers unaffected
- [ ] 2.2 Extend `RegisterDomainRequest` with optional `owningTeam`; replace `IamOnlyAuthorizer` in register.rs with the new OAuth rule (IAM path unchanged); persist `owningTeam` on the domain record
- [ ] 2.3 Red tests: approval accept path commits domain state + owner binding in one transaction; registration without `owningTeam` creates no binding; failure commits neither
- [ ] 2.4 Implement the transactional owner-binding write in the approval accept path
- [ ] 2.5 Green + `make check`; commit `feat(domains): establish team ownership at registration approval`

## 3. Scoped reads and directory projection

- [ ] 3.1 Red tests: team caller list → owned domains only; platform roles → all; IAM unchanged; `taskToken` never in list items
- [ ] 3.2 Implement list scoping via owner bindings; add `?view=directory` names-only projection open to any authenticated caller
- [ ] 3.3 Red tests: describe denied for non-owner non-platform (no existence leak); owner and platform allowed
- [ ] 3.4 Implement describe authorization
- [ ] 3.5 Green + `make check`; commit `feat(domains): scope reads by ownership with names-only directory`

## 4. OAuth update authorization

- [ ] 4.1 Red tests: OAuth update denied without owner binding/platform-admin; allowed for owner team and platform-admin; IAM unchanged
- [ ] 4.2 Replace `IamOnlyAuthorizer` in update.rs with grant-engine owner authorization
- [ ] 4.3 Green + `make check`; commit `feat(domains): authorize OAuth updates via owner bindings`

## 5. Approvals inbox endpoint

- [ ] 5.1 Red tests (handler + api_surfaces): `GET /domains/registrations/pending` returns pending domains with `taskToken`, owningTeam, and owner details; 403 for owner/auditor/IAM callers; empty list → 200
- [ ] 5.2 Implement the handler and wire the route (OAuth surface only) in `nexus-core-stack.yaml` and `api/openapi-oauth.yaml`
- [ ] 5.3 Green + `make check` + cfn-lint; commit `feat(domains): add pending-registrations inbox endpoint`

## 6. Platform enable/disable lifecycle

- [ ] 6.1 Red tests: `POST /domains/{domainName}/state` transitions ACTIVE⇄INACTIVE (platform-admin only); publish and subscribe-activation reject INACTIVE domains; existing estate untouched
- [ ] 6.2 Implement the state endpoint and the INACTIVE guards in the publish and subscribe-activation paths
- [ ] 6.3 Green + `make check`; commit `feat(domains): platform-managed domain enable and disable`

## 7. Producer catalog scope

- [ ] 7.1 Red tests: publisher binding + `?domain=D` (no consumerDomain) returns D's schemas; other domains 403; entitlements not consulted
- [ ] 7.2 Add `Scope::Producer` to list-schemas resolve_scope/sources; same authorization on the detail route
- [ ] 7.3 Green + `make check`; commit `feat(schemas): producer scope for own-domain catalog`

## 8. Cognito client auth flows

- [ ] 8.1 Red test (api_surfaces or template assertion): user-pool client allows `ALLOW_USER_SRP_AUTH` and `ALLOW_REFRESH_TOKEN_AUTH`
- [ ] 8.2 Update `NexusUserPoolClient` in `nexus-core-stack.yaml`; cfn-lint clean
- [ ] 8.3 Commit `feat(auth): enable SRP flows for custom login clients`

## 9. Test personas and docs

- [ ] 9.1 `scripts/seed-users.sh` + `make seed-users`: create `alice@example.com` (platform-admin) and `bob@example.com` (team-payments, creating the group if absent) with permanent passwords from env vars
- [ ] 9.2 Owner-binding backfill script/CLI for pre-existing domains (domain → team-group mapping input) with a dry-run mode
- [ ] 9.3 Update `docs/development/oauth-authentication-guide.md` (SRP login, seed-users) and `docs/development/architecture.md` (owner role, scoped reads, pending endpoint)
- [ ] 9.4 `openspec validate enable-oauth-domain-management --strict` clean; full gates; commit `docs: oauth domain management and test personas`
