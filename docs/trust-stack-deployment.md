# Trust Stack Deployment Guide

The trust stack grants EventBridge Nexus cross-account access to create and manage domain resources (event buses, DLQs, bus-to-bus IAM roles) in an application account.

## Prerequisites

1. The Nexus core stack is deployed in the platform account.
2. You have the following outputs from the Nexus core stack:
   - `NexusFunctionRoleArn` -- the trust-anchor role ARN
   - `ProvisioningWorkflowRoleArn` -- the Step Functions provisioning role ARN
3. You know the `Environment` and `Organization` values matching the Nexus configuration.
4. You have AWS CLI access with permissions to deploy CloudFormation in the target application account.

## Deploy

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <org>-<env>-nexus-trust \
  --parameter-overrides \
    NexusRoleArn=arn:aws:iam::<platform-account-id>:role/<org>-<env>-nexus-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::<platform-account-id>:role/<org>-<env>-provisioning-wf-role \
    Environment=<env> \
    Organization=<org> \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

Replace the `<placeholders>` with actual values. Example for a `dev` environment with organization `acme` and platform account `111111111111`:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name acme-dev-nexus-trust \
  --parameter-overrides \
    NexusRoleArn=arn:aws:iam::111111111111:role/acme-dev-nexus-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::111111111111:role/acme-dev-provisioning-wf-role \
    Environment=dev \
    Organization=acme \
  --capabilities CAPABILITY_NAMED_IAM \
  --region us-east-1
```

## Verify

After deployment, confirm the trust role exists and has the correct trust policy:

```bash
aws iam get-role --role-name <org>-<env>-nexus-trust-role
```

The trust policy should list both the Nexus function role and provisioning workflow role as allowed principals.

## What the Trust Stack Creates

| Resource | Purpose |
|---|---|
| `<org>-<env>-nexus-trust-role` | IAM role the Nexus assumes to manage resources in this account |

The role grants scoped permissions for:
- **EventBridge**: create/delete/describe event buses, manage permissions (scoped to `<org>-<env>-*` buses)
- **SQS**: create/delete DLQ queues (scoped to `<org>-<env>-*` queues)
- **IAM**: create/delete bus-to-bus forwarding roles (scoped to `<org>-<env>-b2b-*` roles)

## Updating the Trust Stack

If the Nexus core stack is redeployed and the role ARNs change, update the trust stack:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <org>-<env>-nexus-trust \
  --parameter-overrides \
    NexusRoleArn=<new-role-arn> \
    ProvisioningWorkflowRoleArn=<new-wf-role-arn> \
    Environment=<env> \
    Organization=<org> \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

## Removing the Trust Stack

Deleting the stack revokes cross-account access. Existing domain resources in the account will remain but the Nexus will no longer be able to manage them.

```bash
aws cloudformation delete-stack --stack-name <org>-<env>-nexus-trust --region <region>
```

## Troubleshooting

| Issue | Resolution |
|---|---|
| `AccessDenied` during domain provisioning | Verify the trust role exists in the target account and the trust policy lists the correct platform account role ARNs |
| Stack fails with `CAPABILITY_NAMED_IAM` error | Add `--capabilities CAPABILITY_NAMED_IAM` to the deploy command |
| Role name conflict | The role name follows `<org>-<env>-nexus-trust-role`. Ensure no pre-existing role with that name exists |
