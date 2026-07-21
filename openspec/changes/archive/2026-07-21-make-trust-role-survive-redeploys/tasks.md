# Tasks: make-trust-role-survive-redeploys

## 1. Trust Stack Template

- [x] 1.1 Rewrite `NexusTrustRole.AssumeRolePolicyDocument` in `trust-stack.yaml`: principal = platform account root (account id derived from the existing role-ARN parameters via `!Select`/`!Split`), `Condition: ArnEquals: aws:PrincipalArn: [NexusRoleArn, ProvisioningWorkflowRoleArn]`
- [x] 1.2 Run cfn-lint / `sam validate --lint` on the template — both pass

## 2. Documentation

- [x] 2.1 Update `docs/trust-stack-deployment.md`: describe the account-root + `aws:PrincipalArn` trust model and note that platform redeploys no longer require touching trust stacks
- [x] 2.2 Update the trust table in `docs/development/architecture.md` to match

## 3. Deploy and Verify

- [x] 3.1 Update the deployed trust stack in the domain account: `make deploy-trust APP_PROFILE=<profile>` — in-place update applied; deployed policy shows account-root principal + `ArnEquals` condition
- [x] 3.2 Verify assumption still works: `make smoke` registered/approved `smoke-test`, provisioned to ACTIVE across dev/staging/prod, then torn down cleanly (empty `compensationErrors`)
- [x] 3.3 Verify the failure scenario is gone: tear down and redeploy the core stack, then provision another throwaway domain WITHOUT touching the trust stack; provisioning SHALL succeed (previously failed with `not authorized to assume`) — verified: core stack torn down and redeployed (new platform roles with different unique IDs), trust stack left untouched, `smoke-test` domain provisioned to ACTIVE across all envs, then torn down cleanly
- [x] 3.4 Confirm least privilege: operator `sts:AssumeRole` attempt denied (`AccessDenied` for `user/sliedig`)
