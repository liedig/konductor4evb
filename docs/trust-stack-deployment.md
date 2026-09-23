# Trust Stack Deployment Guide

The trust stack grants Konductor for Amazon EventBridge cross-account access to create and manage domain resources. These include event buses, dead-letter queues (DLQs), and bus-to-bus AWS Identity and Access Management (IAM) roles.

Deploy one trust role in each domain account. The same role serves every environment of every domain owned by that account.

## Prerequisites

Before deployment, confirm the following prerequisites:

1. The Konductor core stack is deployed in the platform account.
2. You know the `Organization` value matching the Konductor configuration (default: `acme`).
3. You have an AWS Command Line Interface (AWS CLI) profile with permission to deploy AWS CloudFormation stacks in the target domain account.

## Deploy

Use the Makefile target for the standard deployment, or supply parameters directly with the AWS CLI.

### Makefile (recommended)

The `deploy-trust` Makefile target resolves `KonductorFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the deployed `konductor-core` stack outputs. You do not need to copy these Amazon Resource Names (ARNs) manually:

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
  --stack-name konductor-trust \
  --parameter-overrides \
    KonductorRoleArn=arn:aws:iam::<platform-account-id>:role/konductor-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::<platform-account-id>:role/konductor-provisioning-workflow-role \
    Organization=<org> \
  --tags Application=konductor Component=trust \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

Replace `<platform-account-id>`, `<org>`, and `<region>` with your platform account ID, organization identifier, and AWS Region. The following example uses organization `acme` and the fictitious platform account `111111111111`:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name konductor-trust \
  --parameter-overrides \
    KonductorRoleArn=arn:aws:iam::111111111111:role/konductor-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::111111111111:role/konductor-provisioning-workflow-role \
    Organization=acme \
  --tags Application=konductor Component=trust \
  --capabilities CAPABILITY_NAMED_IAM \
  --region us-east-1
```

## Verify

After deployment, confirm the trust role exists and has the correct trust policy:

```bash
aws iam get-role --role-name <org>-konductor-trust-role
```

The trust policy names the platform account root as principal, constrained by an `ArnEquals` condition on `aws:PrincipalArn` to exactly the Konductor function role and the provisioning workflow role. Only those two roles can assume the trust role; the account-root principal alone grants nothing.

IAM resolves role-ARN principals to internal unique IDs when it saves a trust policy. If the core stack recreates a named role, that role receives a new ID. A policy that names the role directly then loses its trust relationship.

The `aws:PrincipalArn` condition instead compares the ARN string on each request. Both platform role names are deterministic, so the trust relationship survives core-stack redeployments.

## What the trust stack creates

| Resource | Purpose |
|---|---|
| `<org>-konductor-trust-role` | IAM role the Konductor assumes to manage resources in this account, across all environments of all domains |

The role grants scoped permissions for:

- **EventBridge**: create/delete/describe event buses, manage permissions (scoped to `<org>-*` buses, covering all envs)
- **SQS**: create/delete DLQ queues (scoped to `<org>-*` queues, covering all envs)
- **IAM**: create/delete bus-to-bus forwarding roles (scoped to `<org>-*-b2b-*` roles, covering all envs)

## Updating the trust stack

Redeploying the Konductor core stack does **not** require any trust stack update: the platform role names are fixed, their ARN strings do not change, and the trust policy's `aws:PrincipalArn` condition keeps matching. Only if the role ARNs themselves change (different platform account, renamed roles) rerun the Makefile target (it auto-fetches the latest ARNs):

```bash
make deploy-trust APP_PROFILE=<aws-cli-profile>
```

Or update manually:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name konductor-trust \
  --parameter-overrides \
    KonductorRoleArn=<new-role-arn> \
    ProvisioningWorkflowRoleArn=<new-workflow-role-arn> \
    Organization=<org> \
  --tags Application=konductor Component=trust \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

## Removing the trust stack

Deleting the stack revokes cross-account access. Existing domain resources remain in the account, but Konductor can no longer manage them.

```bash
make teardown-trust APP_PROFILE=<aws-cli-profile>
```

Or delete manually:

```bash
aws cloudformation delete-stack --stack-name konductor-trust --region <region> --profile <aws-cli-profile>
```

## Troubleshooting

| Issue | Resolution |
|---|---|
| `AccessDenied` during domain provisioning | Verify the trust role exists in the target account and its trust policy's `aws:PrincipalArn` condition lists the correct platform role ARNs |
| Stack fails with `CAPABILITY_NAMED_IAM` error | Add `--capabilities CAPABILITY_NAMED_IAM` to the deploy command |
| Role name conflict | The role name follows `<org>-konductor-trust-role`. Ensure no pre-existing role with that name exists |
