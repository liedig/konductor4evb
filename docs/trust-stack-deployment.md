# Trust Stack Deployment Guide

The trust stack grants EventBridge Nexus cross-account access to create and manage domain resources (event buses, DLQs, bus-to-bus IAM roles) in a domain account. **One trust role per domain account** — the same role serves all environments of all domains owned by that account.

## Prerequisites

1. The Nexus core stack is deployed in the platform account.
2. You know the `Organization` value matching the Nexus configuration (default: `acme`).
3. You have an AWS CLI profile configured with permissions to deploy CloudFormation in the target domain account.

## Deploy

### Makefile (recommended)

The `deploy-trust` Makefile target automatically resolves `NexusFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the deployed `nexus-core` stack outputs, eliminating manual copy-paste of role ARNs:

```bash
make deploy-trust APP_PROFILE=<aws-cli-profile>
```

Override the organization if needed:

```bash
make deploy-trust APP_PROFILE=demo ORGANIZATION=myorg
```

### Manual (alternative)

If you need full control over the deployment parameters, use `aws cloudformation deploy` directly:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <org>-nexus-trust \
  --parameter-overrides \
    NexusRoleArn=arn:aws:iam::<platform-account-id>:role/<org>-nexus-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::<platform-account-id>:role/nexus-provisioning-workflow-role \
    Organization=<org> \
  --tags Application=nexus Component=trust \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

Replace the `<placeholders>` with actual values. Example for organization `acme` and platform account `111111111111`:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name nexus-trust \
  --parameter-overrides \
    NexusRoleArn=arn:aws:iam::111111111111:role/nexus-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::111111111111:role/nexus-provisioning-workflow-role \
    Organization=acme \
  --tags Application=nexus Component=trust \
  --capabilities CAPABILITY_NAMED_IAM \
  --region us-east-1
```

## Verify

After deployment, confirm the trust role exists and has the correct trust policy:

```bash
aws iam get-role --role-name <org>-nexus-trust-role
```

The trust policy names the platform account root as principal, constrained by an `ArnEquals` condition on `aws:PrincipalArn` to exactly the Nexus function role and the provisioning workflow role. Only those two roles can assume the trust role; the account-root principal alone grants nothing.

This shape is deliberate. IAM resolves role-ARN principals to internal unique IDs when a trust policy is saved, so naming the platform roles directly would sever trust whenever the Nexus core stack is torn down and redeployed (the recreated roles get new unique IDs). The `aws:PrincipalArn` condition is string-matched on every request instead, and both platform role names are deterministic, so trust survives platform redeploys without touching this stack.

## What the Trust Stack Creates

| Resource | Purpose |
|---|---|
| `<org>-nexus-trust-role` | IAM role the Nexus assumes to manage resources in this account, across all environments of all domains |

The role grants scoped permissions for:

- **EventBridge**: create/delete/describe event buses, manage permissions (scoped to `<org>-*` buses, covering all envs)
- **SQS**: create/delete DLQ queues (scoped to `<org>-*` queues, covering all envs)
- **IAM**: create/delete bus-to-bus forwarding roles (scoped to `<org>-*-b2b-*` roles, covering all envs)

## Updating the Trust Stack

Redeploying the Nexus core stack does **not** require any trust stack update: the platform role names are fixed, their ARN strings do not change, and the trust policy's `aws:PrincipalArn` condition keeps matching. Only if the role ARNs themselves change (different platform account, renamed roles) re-run the Makefile target (it auto-fetches the latest ARNs):

```bash
make deploy-trust APP_PROFILE=<aws-cli-profile>
```

Or update manually:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <org>-nexus-trust \
  --parameter-overrides \
    NexusRoleArn=<new-role-arn> \
    ProvisioningWorkflowRoleArn=<new-workflow-role-arn> \
    Organization=<org> \
  --tags Application=nexus Component=trust \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

## Removing the Trust Stack

Deleting the stack revokes cross-account access. Existing domain resources in the account will remain but the Nexus will no longer be able to manage them.

```bash
make teardown-trust APP_PROFILE=<aws-cli-profile>
```

Or delete manually:

```bash
aws cloudformation delete-stack --stack-name nexus-trust --region <region> --profile <aws-cli-profile>
```

## Troubleshooting

| Issue | Resolution |
|---|---|
| `AccessDenied` during domain provisioning | Verify the trust role exists in the target account and its trust policy's `aws:PrincipalArn` condition lists the correct platform role ARNs |
| Stack fails with `CAPABILITY_NAMED_IAM` error | Add `--capabilities CAPABILITY_NAMED_IAM` to the deploy command |
| Role name conflict | The role name follows `<org>-nexus-trust-role`. Ensure no pre-existing role with that name exists |
