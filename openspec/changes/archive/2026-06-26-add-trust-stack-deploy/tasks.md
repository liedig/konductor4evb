## 1. Makefile targets

- [x] 1.1 Add `deploy-trust` target with `APP_PROFILE` required guard, `ORGANIZATION` defaulting to `acme`, auto-fetch of `KonductorFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from `konductor-core` stack outputs, and `aws cloudformation deploy` call with `CAPABILITY_NAMED_IAM` and tags
- [x] 1.2 Add `teardown-trust` target with `APP_PROFILE` required guard and `aws cloudformation delete-stack` call
- [x] 1.3 Update `.PHONY` to include `deploy-trust teardown-trust`
- [x] 1.4 Update `help` target to list `deploy-trust` and `teardown-trust` under Deploy and Teardown sections

## 2. Documentation

- [x] 2.1 Update `docs/trust-stack-deployment.md` to present `make deploy-trust APP_PROFILE=<profile>` as the primary deployment method, retaining the manual command as an alternative
- [x] 2.2 Update `docs/development/decisions.md` with decision to auto-fetch role ARNs from core stack outputs instead of manual copy-paste

## 3. Verification

- [x] 3.1 Run `make help` and verify `deploy-trust` and `teardown-trust` appear in the help output
- [x] 3.2 Run `make deploy-trust` without `APP_PROFILE` and verify the error message is clear
- [x] 3.3 Run `make teardown-trust` without `APP_PROFILE` and verify the error message is clear
