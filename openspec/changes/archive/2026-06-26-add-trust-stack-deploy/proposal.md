## Why

Deploying the trust stack in application accounts requires manually running `aws cloudformation deploy` with role ARNs copy-pasted from core stack outputs. This is error-prone, undocumented in the Makefile, and breaks the one-command deployment ergonomics that `make deploy-core` and `make deploy-notifications` already provide. As Konductor onboards more application accounts, a repeatable Makefile target is needed.

## What Changes

- Add `deploy-trust` Makefile target that deploys `trust-stack.yaml` to a target application account
- Add `teardown-trust` Makefile target for trust stack cleanup
- Accept `APP_PROFILE` (AWS CLI profile for the target account) and `ORGANIZATION` as make variables
- Auto-fetch `KonductorFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the deployed `konductor-core` stack outputs, eliminating manual copy-paste
- Update `help` target to include the new trust targets
- Update `docs/trust-stack-deployment.md` to reference the new Makefile target

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `installation`: The trust stack deployment workflow changes from a manual `aws cloudformation deploy` command to a `make deploy-trust` target with automatic role ARN resolution from core stack outputs.

## Impact

- **Makefile**: New `deploy-trust` and `teardown-trust` targets; updated `help` target
- **docs/trust-stack-deployment.md**: Updated to reference `make deploy-trust` as the primary deployment method
- **No code changes**: No Lambda functions, state machines, or CloudFormation templates are modified
- **No breaking changes**: The manual `aws cloudformation deploy` approach continues to work; the Makefile target is a convenience wrapper
