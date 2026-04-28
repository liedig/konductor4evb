# Trust Stack Deployment Guide

The trust stack grants EventBridge Nexus cross-account access to create and manage domain resources (event buses, DLQs, bus-to-bus IAM roles) in an application account. **One trust role per application account** — the same role serves all environments of all domains owned by that account.

## Prerequisites

1. The Nexus core stack is deployed in the platform account.
2. You have the following outputs from the Nexus core stack:
   - `NexusFunctionRoleArn` -- the trust-anchor role ARN
   - `ProvisioningWorkflowRoleArn` -- the Step Functions provisioning role ARN
3. You know the `Organization` value matching the Nexus configuration.
4. You have AWS CLI access with permissions to deploy CloudFormation in the target application account.

## Deploy

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <org>-nexus-trust \
  --parameter-overrides \
    NexusRoleArn=arn:aws:iam::<platform-account-id>:role/<org>-nexus-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::<platform-account-id>:role/nexus-provisioning-workflow-role \
    Organization=<org> \
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
  --capabilities CAPABILITY_NAMED_IAM \
  --region us-east-1
```

## Verify

After deployment, confirm the trust role exists and has the correct trust policy:

```bash
aws iam get-role --role-name <org>-nexus-trust-role
```

The trust policy should list both the Nexus function role and provisioning workflow role as allowed principals.

## What the Trust Stack Creates

| Resource | Purpose |
|---|---|
| `<org>-nexus-trust-role` | IAM role the Nexus assumes to manage resources in this account, across all environments of all domains |

The role grants scoped permissions for:
- **EventBridge**: create/delete/describe event buses, manage permissions (scoped to `<org>-*` buses, covering all envs)
- **SQS**: create/delete DLQ queues (scoped to `<org>-*` queues, covering all envs)
- **IAM**: create/delete bus-to-bus forwarding roles (scoped to `<org>-*-b2b-*` roles, covering all envs)

## Updating the Trust Stack

If the Nexus core stack is redeployed and the role ARNs change, update the trust stack:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <org>-nexus-trust \
  --parameter-overrides \
    NexusRoleArn=<new-role-arn> \
    ProvisioningWorkflowRoleArn=<new-workflow-role-arn> \
    Organization=<org> \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

## Removing the Trust Stack

Deleting the stack revokes cross-account access. Existing domain resources in the account will remain but the Nexus will no longer be able to manage them.

```bash
aws cloudformation delete-stack --stack-name <org>-nexus-trust --region <region>
```

## Troubleshooting

| Issue | Resolution |
|---|---|
| `AccessDenied` during domain provisioning | Verify the trust role exists in the target account and the trust policy lists the correct platform account role ARNs |
| Stack fails with `CAPABILITY_NAMED_IAM` error | Add `--capabilities CAPABILITY_NAMED_IAM` to the deploy command |
| Role name conflict | The role name follows `<org>-nexus-trust-role`. Ensure no pre-existing role with that name exists |
