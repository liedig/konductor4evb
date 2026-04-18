# EventBridge Nexus Installation Guide

This guide walks you through deploying EventBridge Nexus into your own AWS environment from scratch. By the end, you will have a working Nexus installation and will have verified it by registering, approving, and activating a domain.

## Overview

Nexus deploys as three CloudFormation stacks across two types of AWS accounts:

```
┌─────────────────────────────────────────────────┐
│  Platform Account                                │
│                                                  │
│  1. notifications-stack (optional)               │
│     SES email identities for testing             │
│                                                  │
│  2. nexus core stack (required)                  │
│     API Gateway, Lambda functions, DynamoDB,     │
│     Step Functions, EventBridge bus,             │
│     domain event buses, DLQs, dashboards         │
│                                                  │
└──────────────────────┬──────────────────────────┘
                       │ STS AssumeRole
┌──────────────────────▼──────────────────────────┐
│  Application Account (one per team)              │
│                                                  │
│  3. trust stack (required per account)           │
│     IAM role granting Nexus cross-account access  │
│                                                  │
└─────────────────────────────────────────────────┘
```

## Prerequisites

### Local toolchain

| Tool | Install | Verify |
|---|---|---|
| Rust stable | [rustup.rs](https://rustup.rs/) — the project pins stable via `rust-toolchain.toml` | `rustc --version` |
| cargo-lambda | [cargo-lambda.info](https://www.cargo-lambda.info/) | `cargo lambda --version` |
| zig (for cross-compilation) | Comes with cargo-lambda or install via [ziglang.org](https://ziglang.org/) | `zig version` |
| AWS SAM CLI | [docs.aws.amazon.com](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/install-sam-cli.html) | `sam --version` |
| AWS CLI | [docs.aws.amazon.com](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html) | `aws --version` |
| curl / jq | System package manager | `curl --version && jq --version` |
| awscurl (for smoke test) | `pip install awscurl` | `awscurl --version` |

### AWS environment

- **Platform account**: The AWS account where Nexus runs. You need credentials with permissions to deploy CloudFormation stacks, create IAM roles, and manage API Gateway, Lambda, DynamoDB, EventBridge, and SES resources.
- **Application account(s)**: One or more AWS accounts where domain resources will be provisioned. You need credentials with permissions to deploy CloudFormation stacks and create IAM roles.
- **AWS Organizations**: The platform account must belong to an AWS Organization. Nexus uses AWS RAM to share domain namespace information across accounts. Capture the Organization ARN (e.g., `arn:aws:organizations::123456789012:organization/o-abc123`) — you will need it as a parameter.
- **Email addresses** (optional): For approval and rejection notifications during testing. Both the sender and recipient addresses must be verified in SES sandbox mode. If you skip this, you can still approve domains via the API directly.

### Decide on your namespace

Nexus resources follow the `{org}-{env}-{domain}` naming convention. Choose values for:

| Parameter | Example | Notes |
|---|---|---|
| `Organization` | `acme` | Short identifier for your organization |
| `Environment` | `dev` | One of `dev`, `staging`, `prod` |

These values are immutable once deployed. AWS resources within the stack will include them (e.g., `acme-dev-nexus-bus`). Stack names use the organization only (e.g., `acme-nexus`).

---

## Phase 1 — Deploy the notifications stack (optional)

This stack creates SES email identities for approval and rejection notifications. It is for testing against the SES sandbox only. Skip this phase if you do not need email notifications.

**Important:** Deploy this stack in the same AWS region as the Nexus core stack. SES identities are regional.

```bash
aws cloudformation deploy \
  --template-file notifications-stack.yaml \
  --stack-name <org>-nexus-notifications \
  --parameter-overrides \
    FromEmail=<sender@example.com> \
    ToEmail=<recipient@example.com> \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

Example:

```bash
aws cloudformation deploy \
  --template-file notifications-stack.yaml \
  --stack-name acme-nexus-notifications \
  --parameter-overrides \
    FromEmail=nexus-sender@example.com \
    ToEmail=platform-team@example.com \
  --capabilities CAPABILITY_NAMED_IAM \
  --region us-east-1
```

### Verify SES identities

SES sandbox requires both email addresses to be verified before you can send:

1. After deploying, check your inbox for SES verification emails from AWS.
2. Click the verification link in each email.
3. Confirm verification status:

```bash
aws ses get-identity-verification-attributes \
  --identities <sender@example.com> <recipient@example.com> \
  --region <region>
```

Both identities should show `VerificationStatus: Success`.

Capture the email addresses for Phase 2:

```bash
aws cloudformation describe-stacks \
  --stack-name <org>-nexus-notifications \
  --query "Stacks[0].Outputs" \
  --region <region>
```

---

## Phase 2 — Deploy the Nexus core stack

Deploy the core stack into the platform account.

### Build and test locally

```bash
make check    # cargo fmt, check, clippy
make test     # cargo test --workspace
make build    # sam build (Rust cross-compilation for arm64)
```

### Deploy

For a first-time deployment with guided prompts:

```bash
sam deploy --guided
```

For repeatable deployments using a configured profile (e.g., `dev`):

```bash
sam deploy --config-env dev
```

**Required parameters:**

| Parameter | Description | Example |
|---|---|---|
| `Organization` | Organization identifier | `acme` |
| `Environment` | Deployment environment | `dev` |
| `OrganizationArn` | AWS Organization ARN (for RAM sharing) | `arn:aws:organizations::123456789012:organization/o-abc123` |

**Optional parameters:**

| Parameter | Description | Default |
|---|---|---|
| `ApprovalFromEmail` | SES sender for approval notifications (from Phase 1) | `""` (disabled) |
| `ApprovalToEmail` | SES recipient for approval notifications (from Phase 1) | `""` (disabled) |
| `NamespacePattern` | Resource naming convention | `{org}-{env}-{domain}` |

If you completed Phase 1, pass the email addresses:

```bash
sam deploy --config-env dev \
  --parameter-overrides \
    Environment=dev \
    Organization=acme \
    OrganizationArn=arn:aws:organizations::123456789012:organization/o-abc123 \
    ApprovalFromEmail=nexus-sender@example.com \
    ApprovalToEmail=platform-team@example.com
```

### Capture stack outputs

After deployment succeeds, capture the outputs needed for Phase 3:

```bash
aws cloudformation describe-stacks \
  --stack-name <org>-nexus \
  --query "Stacks[0].Outputs" \
  --region <region>
```

You need these two values:

| Output | Used in Phase 3 as |
|---|---|
| `NexusFunctionRoleArn` | `NexusRoleArn` parameter |
| `ProvisioningWorkflowRoleArn` | `ProvisioningWorkflowRoleArn` parameter |

Also note the `ApiEndpoint` output — you will use it in the smoke test.

---

## Phase 3 — Onboard an application account

Deploy the trust stack in each application account that will own domains. This grants Nexus cross-account access to provision domain resources.

Switch your AWS CLI credentials to the application account, then:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <org>-<env>-nexus-trust \
  --parameter-overrides \
    NexusRoleArn=<NexusFunctionRoleArn from Phase 2> \
    ProvisioningWorkflowRoleArn=<ProvisioningWorkflowRoleArn from Phase 2> \
    Environment=<env> \
    Organization=<org> \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

Example:

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

For detailed instructions including verification and troubleshooting, see [trust-stack-deployment.md](trust-stack-deployment.md).

---

## Phase 4 — Smoke test

Verify your installation by running the automated smoke test, which registers a test domain, approves it, verifies ACTIVE status and provisioned resources, then deletes the domain:

```bash
make smoke AWS_REGION=<region> APP_ACCOUNT_ID=<application-account-id> OWNER_EMAIL=<owner@example.com>
```

Example:

```bash
make smoke AWS_REGION=us-east-1 APP_ACCOUNT_ID=222233334444 OWNER_EMAIL=owner@example.com
```

The smoke test:
1. Fetches the API endpoint from the core stack outputs
2. Registers a domain named `smoke-test` via `awscurl` (IAM-signed request)
3. Approves the domain via the API
4. Polls until the domain reaches `ACTIVE` status (up to ~2.5 minutes)
5. Verifies platform-account resources (event bus, schema registry, DLQ)
6. Deletes the test domain and waits for `DELETED` status

If all steps pass, your Nexus installation is working.

### Manual smoke test

If you prefer to run the steps manually (e.g., to test with a real domain name or inspect intermediate states), see the API endpoints below. The API uses IAM authentication — use `awscurl` for signed requests.

```bash
API_URL="<ApiEndpoint from Phase 2>"
REGION="<region>"

# Register
awscurl -X POST "${API_URL}/domains" --region "${REGION}" \
  -H "Content-Type: application/json" \
  -d '{"domainName":"billing","ownerAccountId":"<app-account-id>","ownerEmail":"<email>","description":"Billing domain"}'

# Approve
awscurl -X POST "${API_URL}/domains/registrations/decide" --region "${REGION}" \
  -H "Content-Type: application/json" \
  -d '{"domainName":"billing","decision":"ACCEPT"}'

# Check status
awscurl -X GET "${API_URL}/domains/<domainId>" --region "${REGION}"

# Delete
awscurl -X DELETE "${API_URL}/domains/<domainId>" --region "${REGION}"
```

---

## Troubleshooting

### SES sandbox: verification emails not received

In SES sandbox mode, both the sender and recipient email addresses must be verified. After deploying the notifications stack, AWS sends a verification email to each address. If you don't receive it:

- Check spam/junk folders.
- Confirm the email addresses are correct in the stack parameters.
- Verify status with `aws ses get-identity-verification-attributes`.
- SES sandbox only allows sending to verified addresses. You cannot send to arbitrary recipients until you request production access.

### RAM share failed: not in an AWS Organization

The `OrganizationArn` parameter is required. Nexus uses AWS RAM to share domain namespace information across accounts. If you see errors during provisioning related to RAM:

- Confirm the platform account belongs to an AWS Organization.
- Verify the Organization ARN is correct (`arn:aws:organizations::...`).
- Ensure the application account is in the same Organization.

### Trust role AssumeRole denied

If domain provisioning fails with `AccessDenied` when assuming the trust role:

- Verify the trust stack is deployed in the target application account.
- Check the trust role's trust policy lists the correct platform account role ARNs.
- Confirm the role ARNs match the `NexusFunctionRoleArn` and `ProvisioningWorkflowRoleArn` outputs from the core stack.

```bash
aws iam get-role \
  --role-name <org>-<env>-nexus-trust-role \
  --region <region>
```

### SAM build cache is stale

If builds fail or produce unexpected results after changing code:

```bash
make clean    # removes cargo build artifacts and .aws-sam
make build    # fresh build
```

The `sam build --beta-features -c` command uses a cache (`-c` flag). If you encounter build issues, clearing the cache with `make clean` is the first step.

### DynamoDB table retained after stack delete

The `DomainsTable` has `DeletionPolicy: Retain`. When you delete the core stack, the DynamoDB table is preserved. To fully remove it:

```bash
aws dynamodb delete-table \
  --table-name <org>-<env>-domains \
  --region <region>
```

---

## Uninstall

To remove a Nexus installation completely:

### 1. Delete all domains

Before deleting the stacks, remove all domains to clean up provisioned resources (event buses, DLQs, dashboards) in both accounts:

```bash
# List all domains
awscurl -X GET "${API_URL}/domains" --region "${REGION}"

# Delete each domain
awscurl -X DELETE "${API_URL}/domains/<domainId>" --region "${REGION}"
```

Wait for each domain to reach `DELETED` status. The cleanup workflow runs automatically.

### 2. Delete the core stack

```bash
aws cloudformation delete-stack \
  --stack-name <org>-nexus \
  --region <region>
```

### 3. Delete trust stacks (in each application account)

Switch credentials to each application account:

```bash
aws cloudformation delete-stack \
  --stack-name <org>-<env>-nexus-trust \
  --region <region>
```

### 4. Delete the notifications stack

```bash
aws cloudformation delete-stack \
  --stack-name <org>-nexus-notifications \
  --region <region>
```

### 5. Clean up retained resources

The following resources are not automatically deleted when their stacks are removed:

- **DynamoDB table**: `<org>-<env>-domains` (retained by `DeletionPolicy`)
- **CloudWatch log groups**: Under `/aws/lambda/<org>-<env>-*` and `/aws/apigateway/<org>-<env>-*`

```bash
# Delete the retained DynamoDB table
aws dynamodb delete-table --table-name <org>-<env>-domains --region <region>

# List and delete log groups
aws logs describe-log-groups \
  --log-group-name-prefix "/aws/lambda/<org>-<env>" \
  --region <region> \
  --query "logGroups[*].logGroupName" --output text

aws logs delete-log-group --log-group-name <log-group-name> --region <region>
```
