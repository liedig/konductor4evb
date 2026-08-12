# Design: make-trust-role-survive-redeploys

## Context

Each domain account runs a trust stack whose single role (`{org}-konductor-trust-role`) is assumed by two platform roles: `konductor-function-role` (Lambda execution) and `konductor-provisioning-workflow-role` (Step Functions provisioning/deprovisioning). The trust policy currently lists those role ARNs as principals. IAM resolves role-ARN principals to internal unique principal IDs (`AROA...`) when the policy is saved; deleting and recreating a role (same name, new unique ID) leaves the stored principal dangling and assumption fails.

Observed failure: after a core-stack teardown/redeploy, all `provision-env` executions failed with `konductor-provisioning-workflow-role is not authorized to assume ... acme-konductor-trust-role`. Recovery required deleting and redeploying the trust stack, because a plain `cloudformation deploy` sees no template/parameter change (role names are fixed, so ARNs are identical strings) and applies nothing.

Constraints: platform and domain accounts belong to the same organization; the platform stack must remain freely re-deployable during development; both platform role names are deterministic (`RoleName` set explicitly in `konductor-core-stack.yaml`), so their ARN strings are stable.

## Goals / Non-Goals

**Goals:**

- Trust between domain accounts and the platform survives platform role deletion/recreation.
- Assumption stays restricted to exactly the two platform roles (no broadening).
- `make deploy-trust` interface unchanged; recovery from the current broken state is a plain stack update.

**Non-Goals:**

- Changing which principals may assume, or adding operator access to the trust role (the backfill descope removed the only would-be operator consumer).
- Retiring the unused `BusToBusRoleManagement` policy (tracked separately as a MAY in the trust-model spec).
- Organization-wide controls (SCPs, permission boundaries).

## Decisions

**D1: Account-root principal + `ArnEquals` on `aws:PrincipalArn`, not role-ARN principals.**
Conditions are evaluated as strings on every `sts:AssumeRole` request, never snapshotted to unique IDs, so role recreation is invisible to the trust policy. The account-root principal only delegates the *possibility* of assumption to the platform account; a platform principal must still carry its own `sts:AssumeRole` allow (the two roles do) and match the condition. Security is equivalent: the only scenario the old behavior defended against (attacker deletes and recreates a platform role) already presumes full platform-account IAM compromise.

Alternatives rejected:
- Document the teardown/redeploy dance: tribal-knowledge tax; failure is silent until provisioning breaks.
- `ArnLike` with `role/konductor-*`: grants assumption to every future platform role; broader than needed.
- `aws:PrincipalArn` for assumed-role sessions resolves to the **role ARN**, not the session ARN, so exact `ArnEquals` matching works without wildcards.

**D2: Derive the platform account id from the existing role-ARN parameters.**
`!Select [4, !Split [":", <RoleArn>]]` yields the account id, so no new parameter and no `make deploy-trust` change. Alternative (new `PlatformAccountId` parameter) rejected: it can drift from the role ARNs and complicates the deploy target for no benefit.

## Risks / Trade-offs

- [Trust no longer auto-revokes if a platform role is deleted permanently] → acceptable: the condition still names the exact ARNs; a deleted role cannot authenticate, and recreation requires platform IAM access. Revocation is achieved by updating or deleting the trust stack.
- [Typo in condition ARNs would silently deny assumption] → the verification task exercises a full core-stack redeploy plus provisioning round-trip before the change is considered done.

## Migration Plan

1. Update `trust-stack.yaml`; run `make deploy-trust APP_PROFILE=<profile>` per domain account. The template differs, so CloudFormation updates the trust policy in place (no teardown, no downtime; in-flight sessions unaffected).
2. Verify: tear down and redeploy the core stack, then register/approve a domain and confirm provisioning assumes the trust role without touching the trust stack.
3. Rollback: redeploy the previous template (restores role-ARN principals; requires the platform roles to exist at that moment).

## Open Questions

(none)
