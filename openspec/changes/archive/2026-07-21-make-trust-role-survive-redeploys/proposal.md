# Proposal: make-trust-role-survive-redeploys

## Why

The trust role's assume-role policy names the platform roles by ARN, which IAM resolves to internal unique principal IDs at save time. Tearing down and redeploying the core stack recreates those roles with new unique IDs, silently severing trust in every domain account: provisioning and deprovisioning fail with `not authorized to assume` until each trust stack is deleted and redeployed (a plain stack update is a no-op because the template and parameters are unchanged). This bit us during dev-stack verification of `add-event-subscriptions` and will bite every future platform redeploy.

## What Changes

- Rewrite the trust role's `AssumeRolePolicyDocument` in `trust-stack.yaml`: principal becomes the platform account root, constrained to the exact platform role ARNs via an `ArnEquals` condition on `aws:PrincipalArn`. Conditions are string-matched per request (never snapshotted to unique IDs), so trust survives role recreation.
- The platform account id is derived from the role ARN parameters the template already receives; the `make deploy-trust` interface is unchanged.
- Update `docs/trust-stack-deployment.md` to explain the trust model and remove the need for teardown/redeploy choreography after platform redeploys.
- No change to who can assume: exactly `konductor-function-role` and `konductor-provisioning-workflow-role` (both fixed-name, so their ARN strings are stable across redeploys).

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `cross-account-trust-model`: the trust role's assumption contract changes from role-ARN principals (unique-ID pinned) to account-root principal + `aws:PrincipalArn` condition (string-matched); adds the requirement that trust survives platform role recreation.

## Impact

- `trust-stack.yaml`: one statement rewritten (AssumeRolePolicyDocument).
- `docs/trust-stack-deployment.md` and `docs/development/architecture.md` (trust table): updated trust-model description.
- Deployed trust stacks: one in-place `make deploy-trust APP_PROFILE=<profile>` per domain account (the template changes, so CloudFormation applies it; no teardown, no downtime, existing sessions unaffected).
- No Rust code, API, or workflow changes.
