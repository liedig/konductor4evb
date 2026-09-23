# Konductor is a singleton per org—drop `Environment` from platform resources

[Documentation home](../README.md)

- Recorded date: 2026-04-24
- Change ID: `konductor-platform-drop-environment`
- Area: Platform
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Konductor itself does not have a `dev`/`staging`/`prod` concept—it is an integration platform deployed once per organization. The `Environment` SAM parameter on the core stack was a category error: it belongs one layer down, to the **domains Konductor provisions**.

**Template rename**: `template.yaml` → `konductor-core-stack.yaml` to match the sibling-stack naming convention (`notifications-stack.yaml`, `trust-stack.yaml`) and make the file's purpose explicit.

**samconfig split**: single `samconfig.toml` split into `samconfig-core.toml` and `samconfig-notifications.toml`—each stack now has an isolated deploy config invoked with `--config-file`.

**Platform resource rename**: all `${Organization}-${Environment}-<thing>` resource names changed to the fixed `konductor-<thing>` pattern (konductor bus, domains table, DLQs, IAM roles, API Gateway, every Lambda + log group, state machines, EventBridge rules)—Konductor is a singleton per AWS account, so neither org nor env prefix is needed. The domain-account trust role is the one exception (`${Organization}-konductor-trust-role`, kept org-scoped so multiple orgs sharing a domain account don't collide). Domain resources (`{org}-{env}-{domain}-*`) keep env scoping because a single domain legitimately has per-env resource sets.

**API stage**: `StageName` becomes static `v1` (no longer env-indexed). **`ENVIRONMENT` Lambda var removed**—grep-confirmed no Rust consumer.

**Reserved name**: `konductor` is now rejected in the register-domain validation path so no domain can collide with the platform namespace.

**Makefile hardening**: `AWS_REGION` defaults via `aws configure get region`; `$(error …)` guards on `APP_ACCOUNT_ID`, `OWNER_EMAIL` (smoke), and `NOTIFICATION_SENDER_EMAIL`, `PLATFORM_OPERATOR_EMAIL` (deploy-notifications). **Breaking for existing deployments**—every platform resource is renamed, so CloudFormation treats it as delete+create. Acceptable since Konductor is pre-production.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-04-27-konductor-platform-drop-environment/proposal.md). The recorded date above is not necessarily the archive or merge date.

## Current relevance

The static `v1` API stage was superseded by [environment-named publisher stages](add-publisher-apis.md). The singleton platform boundary is a separate decision.

[Decision index](README.md)
