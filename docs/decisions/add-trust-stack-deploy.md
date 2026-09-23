# Makefile targets for trust stack deploy/teardown with auto-fetched role ARNs

[Documentation home](../README.md)

- Recorded date: 2026-06-26
- Change ID: `add-trust-stack-deploy`
- Area: Installation / DevEx
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Added `deploy-trust` and `teardown-trust` Makefile targets. `deploy-trust` auto-fetches `KonductorFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the deployed `konductor-core` stack outputs using the platform account credentials, eliminating manual copy-paste of role ARNs. Uses `aws cloudformation deploy` (not `sam deploy`) since `trust-stack.yaml` is plain CloudFormation with no SAM transforms. Accepts `APP_PROFILE` (required, AWS CLI profile for the target domain account) and `ORGANIZATION` (optional, defaults to `acme` matching `samconfig-core.toml`). Stack named `konductor-trust` for consistency with `konductor-core`/`konductor-notifications`. `docs/trust-stack-deployment.md` updated to reference the Makefile target as the primary deployment method, retaining the manual `aws cloudformation deploy` command as an alternative for advanced use cases.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-06-26-add-trust-stack-deploy/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
