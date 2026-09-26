# Platform configuration contract, preflight validation, and Cognito-native access

[Documentation home](../README.md)

- Recorded date: 2026-09-24
- Change ID: `configure-customer-platform` (proposal drafted 2026-09-11)
- Area: Deployment, access administration
- Status: accepted

## Context

The platform is configured entirely through core-stack deployment parameters,
but three gaps surrounded them. The contract was implicit: syntax rules lived
in runtime validators and service quotas, not in any published statement, and
parameters like `Organization` or `NamespacePattern` had no deploy-time
constraints. Customer values—a real AWS Organization ARN and email
addresses—were committed in `samconfig-core.toml`. And a stated customer
concern, keeping platform-access administration under customer control, was
only informally argued, not pinned.

## Decision

1. **Deploy-time contract.** Deployment parameters are the single source of
   truth; there is no runtime settings store. The contract—every setting, its
   syntax, default, and lifecycle class—is published in
   [docs/reference/configuration.md](../reference/configuration.md), with
   bounds derived from the existing runtime validators and service quotas
   (IAM role-name limit, API Gateway stage quota, SQS name charset).
2. **Lifecycle classes.** `Organization`, `Environments`,
   `NamespacePattern`, and `AwsOrganizationArn` are fixed at setup; everything
   else is updateable by redeploy. A drift guard in preflight refuses
   fixed-at-setup changes against a deployed stack with a redeploy message
   instead of letting CloudFormation rename, replace, or orphan resources.
3. **Two-layer validation.** CloudFormation constraints (`AllowedPattern`,
   length bounds) for what the template can verifiably express; a preflight
   (`make validate-config`, `scripts/validate-config.py`) for what it cannot:
   environment uniqueness and count, namespace placeholder presence, SES
   identity verification (both addresses verified, or both empty meaning
   notifications disabled), HTTPS for OAuth URLs outside localhost, and the
   drift guard. `make deploy-core` renders parameter overrides from the same
   validated parse, so deploy and validation cannot read different values.
4. **Customer values out of git.** `konductor-platform.toml` is gitignored;
   a committed example documents every setting; `samconfig-core.toml` and
   `samconfig-notifications.toml` keep only non-customer deploy settings.
   Existing installations generate the file from deployed stack parameters
   (`--init-from-stack`).
5. **Cognito-native access administration.** Platform global-role membership
   is managed only in Amazon Cognito by an authorized operator. No API route,
   binding, or entitlement can express or grant a platform role; an invariant
   test (`lib/konductor-common/tests/platform_role_isolation.rs`) pins the
   closed role model and the absence of group-mutation code paths and IAM
   permissions. Because `platform-admin` resolves from the controlled Cognito
   group and never from the grant table, the platform cannot lock itself out
   (break-glass) and no caller can self-escalate through the API.

## Alternatives considered

- **Runtime-mutable settings store.** Rejected: every consumer treats these
  values as deployment constants (resource names, API stages, Lambda
  environment), so runtime mutability would add a second authority with no
  consumer.
- **In-place environment-list changes.** Rejected: adding or removing an
  environment on a live platform is a provisioning-shaped migration (per-env
  resources, RAM shares, per-domain state), not a parameter edit. Deferred to
  a future `extend-platform-environments` change; most organizations have
  fixed environments.
- **Bootstrap tooling for the first platform administrator.** Rejected:
  Cognito-native administration (console or CLI) is sufficient; the
  `seed-users` script remains development-only.

## Consequences and limits

- Changing a fixed-at-setup setting means a new deployment; the drift guard
  blocks the quiet path and names the honest one.
- Preflight SES and drift checks are read-only AWS calls made with the
  operator's credentials in the deploy region; `--skip-aws-checks` exists for
  offline validation and is not used by `make deploy-core`.
- The real AWS Organization ARN no longer appears in tracked deploy
  configuration. Archived OpenSpec evidence retains historical account and
  email identifiers as records of what was run; they are not deploy
  configuration.
- Verification was fixture-based (recorded stack descriptions, injected
  invariant violations, hermetic TOML cases); this decision does not claim a
  live deployment.

## Evidence

[OpenSpec change](../../openspec/changes/configure-customer-platform/proposal.md)
(design D1–D6, spec deltas, tasks). Invariant mutation check: the scan tests
fail when a group-mutation call or IAM action is injected.

[Decision index](README.md)
