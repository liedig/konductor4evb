# Deferred decisions and follow-ups

[Documentation home](../README.md)

These statuses are preserved from the old decision log, not re-triaged. A later change can close part of a topic without closing the entire deferral. In particular, append-time compatibility hardening does not establish a promotion workflow.

## Recorded deferrals

The following topics record remaining work and updates to earlier deferrals.

| Topic | Dependency | Status |
|-------|-----------|--------|
| Production authorization engine | `evaluate-authorization-engine` spike (conditionally ready) | Shipped: `add-grant-engine` (engine + bindings administration) and `add-subscriber-entitlements` (entitlements + consumer-path migration). Enterprise federation (SAML/OIDC IdP chaining) and group-customization guidance remain deferred to a follow-on change. |
| OAuth UI / console | Cognito user pool + OAuth app client are provisioned | Shipped: `add-konductor-web-ui` (Nuxt console on S3/CloudFront with Cognito login, approvals inbox, domain and subscription journeys). |
| IAM API invocation scoping | None (hardening) | Deferred from the PR #12 code review. `KonductorIamApi` has no API Gateway resource policy, so any same-account principal with `execute-api:Invoke` can reach the automation surface (cross-account invocation is denied by default without a policy). Follow-up: add a resource policy scoped to platform roles. Until then, `update.rs` intentionally lets IAM principals update any domain—the IAM surface is the platform-automation boundary. |
| Gated schema promotion (`gate-schema-promotion`) | `validate-at-ingest` (admission-time validation); subscription version-pinning (shipped) | **Shipped 2026-10-19** by [promote-schema-versions](promote-schema-versions.md): explicit replay-based promotion with aligned version numbers, authoring restricted to the first configured environment, and widen/drain/narrow re-pinning of subscription rules. The append-time compatibility gate stays as resolved below. Original record: Parked 2026-09-02. Design intent: versions are authored in dev and *promoted* between environments; promotion re-pins every subscription's rule filter via `PutRule` on the same rule name (the "version migration is a pattern update" scenario in `event-subscriptions`) and notifies consumers. Open threads: dev-only authoring (direct staging/prod appends become a breaking API change), promote authorization (publisher binding vs a release grant), notification timing (heads-up at authoring vs confirmation at promotion), and compatibility-check placement. **Check placement resolved by incident (2026-09-02)**: during the `schema-authoring-ui` smoke test, an accidental v2→v3 append *removed* a field (`channel`)—a breaking change the append API accepted silently, moving "latest" backwards in contract terms. The strict mechanical rule (no removed/renamed/retyped properties, no new required fields) belongs at **append time** (fail fast where the mistake happens), with promote as a second hard gate. Until promotion ships, every append is effectively promoted immediately, so the append-time check carries the whole weight. |

## Additional known limits

- [Compatibility hardening](harden-schema-compatibility-checker.md) retains the pairwise append-concurrency limit.
- [Durability migrations and idempotency follow-ups](../development/side-effects/README.md) retain their original IDs.
- [Documentation discrepancies](../reference/documentation-discrepancies.md) require separate review, not automatic implementation.
- [Active OpenSpec changes](../../openspec/changes/) track work independently of this index.
