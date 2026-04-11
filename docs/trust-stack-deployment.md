# Trust Stack Deployment Guide

The trust stack grants the EventBridge Control Plane cross-account access to create and manage domain resources (event buses, DLQs, bus-to-bus IAM roles) in an application account.

## Prerequisites

1. The control plane stack is deployed in the platform account.
2. You have the following outputs from the control plane stack:
   - `ControlPlaneFunctionRoleArn` -- the Lambda execution role ARN
   - `ProvisioningWorkflowRoleArn` -- the Step Functions provisioning role ARN (found under the provisioning workflow role in the platform account)
3. You know the `Environment` and `Organization` values matching the control plane configuration.
4. You have AWS CLI access with permissions to deploy CloudFormation in the target application account.

## Deploy

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <env>-<org>-cp-trust-stack \
  --parameter-overrides \
    ControlPlaneRoleArn=arn:aws:iam::<platform-account-id>:role/<env>-<org>-cp-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::<platform-account-id>:role/<env>-<org>-provisioning-wf-role \
    Environment=<env> \
    Organization=<org> \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

Replace the `<placeholders>` with actual values. Example for a `dev` environment with organization `acme` and platform account `111111111111`:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name dev-acme-cp-trust-stack \
  --parameter-overrides \
    ControlPlaneRoleArn=arn:aws:iam::111111111111:role/dev-acme-cp-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::111111111111:role/dev-acme-provisioning-wf-role \
    Environment=dev \
    Organization=acme \
  --capabilities CAPABILITY_NAMED_IAM \
  --region us-east-1
```

## Verify

After deployment, confirm the trust role exists and has the correct trust policy:

```bash
aws iam get-role --role-name <env>-<org>-cp-trust-role
```

The trust policy should list both the control plane function role and provisioning workflow role as allowed principals.

## What the Trust Stack Creates

| Resource | Purpose |
|---|---|
| `<env>-<org>-cp-trust-role` | IAM role the control plane assumes to manage resources in this account |

The role grants scoped permissions for:
- **EventBridge**: create/delete/describe event buses, manage permissions (scoped to `<env>-<org>-*` buses)
- **SQS**: create/delete DLQ queues (scoped to `<env>-<org>-*` queues)
- **IAM**: create/delete bus-to-bus forwarding roles (scoped to `<env>-<org>-b2b-*` roles)

## Updating the Trust Stack

If the control plane is redeployed and the role ARNs change, update the trust stack:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <env>-<org>-cp-trust-stack \
  --parameter-overrides \
    ControlPlaneRoleArn=<new-role-arn> \
    ProvisioningWorkflowRoleArn=<new-wf-role-arn> \
    Environment=<env> \
    Organization=<org> \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

## Removing the Trust Stack

Deleting the stack revokes cross-account access. Existing domain resources in the account will remain but the control plane will no longer be able to manage them.

```bash
aws cloudformation delete-stack --stack-name <env>-<org>-cp-trust-stack --region <region>
```

## Troubleshooting

| Issue | Resolution |
|---|---|
| `AccessDenied` during domain provisioning | Verify the trust role exists in the target account and the trust policy lists the correct platform account role ARNs |
| Stack fails with `CAPABILITY_NAMED_IAM` error | Add `--capabilities CAPABILITY_NAMED_IAM` to the deploy command |
| Role name conflict | The role name follows `<env>-<org>-cp-trust-role`. Ensure no pre-existing role with that name exists |
