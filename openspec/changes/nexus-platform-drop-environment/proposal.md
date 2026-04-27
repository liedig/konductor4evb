## Why

Nexus is deployed once per organisation into a single platform account. It is not an application that lives in `dev`, `staging`, and `prod` — it is the integration platform itself. Despite that, the SAM template, trust stack, samconfig, and IAM role names all carry an `Environment` parameter (defaulting to `dev`) and weave it into every resource name: `acme-dev-nexus-bus`, `acme-dev-nexus-trust-role`, `acme-dev-domains`, etc.

This is a category error. The `Environment` concept belongs one layer down — to the **domains Nexus provisions** (a Sales domain is legitimately deployed across `[dev, staging, prod]`). By conflating the two, the platform templates create the false impression that Nexus itself needs environment scoping, pollute every ARN with a meaningless `-dev-` segment, and make the multi-environment domain feature harder to reason about.

The Makefile also has unguarded `$(AWS_REGION)` / `$(APP_ACCOUNT_ID)` / `$(OWNER_EMAIL)` / `$(FROM_EMAIL)` / `$(TO_EMAIL)` references with no defaults or validation — first-run failures produce confusing AWS CLI errors rather than clear "variable X is required" messages. `AwsOrganizationArn` is a required template parameter with no samconfig default, so `make deploy-core` is broken out-of-the-box.

**Ordering**: this change lands **after** `multi-env-domain-provisioning`. That prior change removes the last real use of `${Environment}` — the provisioning state machine's `DefinitionSubstitutions` — by sourcing env from the domain record per Map iteration. It also pulls forward the one cross-account rename that multi-env hard-depends on: the trust-role rename to `{org}-nexus-trust-role`. By the time this change runs, `${Environment}` in `nexus-core-stack.yaml` is dead code with no consumers.

## What Changes

- Drop the `Environment` parameter from `nexus-core-stack.yaml` and `samconfig-core.toml`. (The `trust-stack.yaml` parameter is already removed by `multi-env-domain-provisioning`.)
- Rename all platform-account resource names from `${Organization}-${Environment}-<thing>` to `${Organization}-nexus-<thing>`.
- API Gateway `StageName` becomes static `v1` (in place of `!Ref Environment`).
- Drop the `ENVIRONMENT` environment variable from `Globals.Function.Environment.Variables` (no Lambda consumers exist).
- Add `AwsOrganizationArn` to samconfig `parameter_overrides` so `make deploy-core` works without manual overrides.
- Add `AWS_REGION` default (via `aws configure get region` with an `us-east-1` fallback) to the Makefile.
- Add `$(error …)` guards on the other required Makefile variables (`APP_ACCOUNT_ID`, `OWNER_EMAIL`, `FROM_EMAIL`, `TO_EMAIL`) so missing values fail loudly with a clear message.
- Retain the `NamespacePattern` parameter but re-scope its documented meaning: it controls **domain resource naming**, not platform resource naming. Platform resources use the fixed `${Organization}-nexus-*` pattern.
- Update `docs/installation.md` and `docs/development/architecture.md` to reflect the new naming and the clarified platform-vs-domain boundary. (`docs/trust-stack-deployment.md` is already updated by `multi-env-domain-provisioning`.)
- Update the `smoke` Makefile target's hardcoded resource names (`acme-dev-smoke-test-*`) to use the current domain naming (still env-scoped at the domain level).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `system-architecture`: The IaC requirement currently states "All resource names SHALL follow the `{org}-{env}-{domain}` naming convention" — this is wrong for platform resources. Updated to distinguish platform resources (`{org}-nexus-*`, fixed) from domain resources (which retain env scoping). The control-plane event bus naming scenario and multi-environment-support scenario are corrected.
- `resource-topology`: Platform resources no longer include an env segment in their names. Domain resources (buses, registries, DLQs) retain env scoping because a single domain has per-env resources.
- `installation`: Installation guide parameters no longer include `Environment` for Nexus stacks. Smoke-test walkthrough reflects current resource naming.
- `domain-provisioning-workflow`: Nexus-internal bus references in the provisioning workflow use `{org}-nexus-bus` (platform resource rename). Trust-role references are already updated by `multi-env-domain-provisioning`.

Trust-model changes (role rename to `{org}-nexus-trust-role`, dropping the `Environment` parameter from `trust-stack.yaml`, rescoped IAM policies) are delivered by the preceding `multi-env-domain-provisioning` change and are not in scope here.

## Impact

- **Breaking for existing deployments.** Every platform and trust-stack resource is renamed, which CloudFormation treats as delete+create. Since Nexus is pre-production, this is acceptable; no migration path is provided.
- **Does not change domain-resource naming.** Domain buses, schema registries, DLQs, and alarms remain named `{org}-{env}-{domain}-*` — env scoping at the domain level is correct and stays untouched by this change.
- **Follows** the `multi-env-domain-provisioning` change, which already removed env references from the provisioning state machine and renamed the trust role. This change removes the last remaining `Environment` references from the Nexus platform templates themselves.
- **Files touched:** `nexus-core-stack.yaml`, `samconfig-core.toml`, `Makefile`, `docs/installation.md`, `docs/development/architecture.md`, `docs/development/decisions.md` (new entry), `openspec/specs/*/spec.md` (via archive).
- **Not touched:** `notifications-stack.yaml` already has no env concept; `trust-stack.yaml` was already handled by multi-env; the provisioning state machine no longer has any env substitution; Rust Lambda code has no `ENVIRONMENT` consumer.
