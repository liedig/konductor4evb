## Context

The Konductor Makefile provides `deploy-core` and `deploy-notifications` targets for platform-account stacks, but trust stack deployment in application accounts requires a manual `aws cloudformation deploy` command with role ARNs copy-pasted from core stack outputs. The trust stack (`trust-stack.yaml`) is a plain CloudFormation template (no SAM transforms) that creates a cross-account IAM role in the target application account.

## Goals / Non-Goals

**Goals:**

- One-command trust stack deployment via `make deploy-trust`
- Automatic resolution of `KonductorFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the deployed `konductor-core` stack outputs
- Support for any application account via an AWS CLI profile variable
- Symmetric `teardown-trust` target for cleanup
- Update `docs/trust-stack-deployment.md` to reference the Makefile target as the primary method

**Non-Goals:**

- Orchestrating multi-account trust deployment in a single command (one account per invocation)
- Modifying the `trust-stack.yaml` template itself
- Adding a samconfig file for the trust stack (unnecessary for plain CloudFormation)
- Automating SES identity verification or domain registration

## Decisions

### 1. Use `aws cloudformation deploy` instead of `sam deploy`

The trust stack is a plain CloudFormation template with no SAM transforms. Using `aws cloudformation deploy` avoids needing a samconfig file and is consistent with the manual approach documented in `docs/trust-stack-deployment.md`.

**Alternative considered:** `sam deploy` with a dedicated samconfig. Rejected because it adds unnecessary configuration for a template that doesn't use SAM features.

### 2. Auto-fetch role ARNs from core stack outputs

The target fetches `KonductorFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the `konductor-core` stack using `aws cloudformation describe-stacks` with the platform profile (default). This eliminates the manual copy-paste step that is the primary source of deployment errors.

**Alternative considered:** Accept role ARNs as make variables. Rejected because it shifts the burden back to the operator and defeats the purpose of automation.

### 3. `APP_PROFILE` variable for target account targeting

The target uses `--profile $(APP_PROFILE)` to deploy into the application account. This follows the existing pattern where `deploy-notifications` uses environment variables for configuration. `APP_PROFILE` is required (no default) to prevent accidental deployment to the wrong account.

### 4. `ORGANIZATION` defaults to `acme`

The `ORGANIZATION` variable defaults to `acme`, matching the value in `samconfig-core.toml`. Operators can override it with `ORGANIZATION=myorg make deploy-trust APP_PROFILE=demo`.

### 5. Stack name uses `konductor-trust` (no org prefix)

The trust stack is named `konductor-trust` for consistency with `konductor-core` and `konductor-notifications`. The org prefix is applied to the role name inside the template, not the stack name.

## Risks / Trade-offs

- **[Risk] Core stack not deployed before trust stack** -> The `deploy-trust` target calls `aws cloudformation describe-stacks --stack-name konductor-core` to fetch outputs. If the core stack doesn't exist, this fails with a clear error. No additional guard needed; the AWS CLI error is self-explanatory.
- **[Risk] SSO temporary credentials expire mid-deployment** -> Operator must re-run `aws sso login` if credentials expire. This is an operational concern outside the Makefile's scope.
- **[Trade-off] One account per invocation** -> Operators deploying trust stacks to many accounts must run the target once per account. This is acceptable for the current scale (tens of accounts) and keeps the target simple.
