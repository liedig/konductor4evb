# Tasks

## 1. Contract and template constraints

- [x] 1.1 Publish the configuration contract table (setting, syntax, default, lifecycle class) from design D2; verify it covers every customer parameter the core template declares.
- [x] 1.2 Add template constraints: `AllowedPattern` and length bounds on `Organization`, a required-placeholder pattern on `NamespacePattern`; verify `sam validate` accepts the template and that invalid values fail at deploy time.
- [x] 1.3 Keep `AccessTokenValidityMinutes` bounds and the `NotificationsEnabled` condition unchanged; verify no regression to existing parameter behavior.

## 2. Preflight validation (`make validate-config`)

- [x] 2.1 Implement the preflight script reading `konductor-platform.toml`: per-entry environment syntax, uniqueness, and count; namespace placeholders; organisation identifier syntax; AWS Organization ARN format; verify each failure message names the setting and the rule.
- [x] 2.2 Add prerequisite checks: SES verification status for both notification addresses (both verified, or both empty reported as notifications disabled) and the HTTPS rule for OAuth URLs outside local development; verify actionable output for each failure.
- [x] 2.3 Implement the drift guard: diff configured values against the deployed stack's parameters and refuse fixed-at-setup differences with the redeploy message; verify against a deployed dev stack or a recorded stack-description fixture.
- [x] 2.4 Wire `make deploy-core` to run preflight first and to source parameter overrides from the configuration file; verify deploy and validation cannot read different values.

## 3. Configuration file handling

- [x] 3.1 Add `konductor-platform.toml.example` documenting every setting; gitignore the real `konductor-platform.toml`.
- [x] 3.2 Remove customer values from the committed `samconfig-core.toml` (keep non-customer deploy settings); verify the repository no longer carries an AWS Organization ARN or email addresses.
- [x] 3.3 Document the existing-installation transition (generate the file from deployed stack parameters) in `docs/operations/installation.md`; verify the steps against a real or recorded stack description.

## 4. Access administration invariant

- [x] 4.1 Add a test pinning that bindings and entitlements cannot express platform global roles and that no API route mutates Cognito group membership; verify the test fails if such a path is introduced.
- [x] 4.2 Update `docs/operations/access-administration.md` to state Cognito-native membership management and the break-glass property; verify placement against `docs/development/documentation.md`.

## 5. Documentation and records

- [x] 5.1 Update the affected pages under `docs/operations/` (installation) and `docs/reference/` (resources, if naming claims move); verify links and anchors with the documentation validation.
- [x] 5.2 Record the lifecycle classification and rejected alternatives in `docs/decisions/configure-customer-platform.md`, indexed in `docs/decisions/README.md`; preserve the 2026-09-11 proposal date.
- [x] 5.3 Add a `CHANGELOG.md` Unreleased entry linking to the decision record; do not claim deployment or acceptance evidence.
- [x] 5.4 Run `openspec validate configure-customer-platform --strict` and the configured Markdown checks; verify no live AWS operations are performed without separate authorization.
