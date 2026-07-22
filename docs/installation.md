# EventBridge Nexus Installation Guide

This guide walks you through deploying EventBridge Nexus into your own AWS environment from scratch. By the end, you will have a working Nexus installation and will have verified it by registering, approving, and activating a domain.

## Overview

Nexus deploys as three CloudFormation stacks across two types of AWS accounts:

```text
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
│  Domain Account (one per team)                   │
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
| zig 0.14.x (for cross-compilation) | `brew install zig@0.14` or install via [ziglang.org](https://ziglang.org/). **zig 0.16+ has a broken `ar` command** — see [Troubleshooting](#zig-016-ar-regression-breaks-sam-build) | `zig version` |
| AWS SAM CLI | [docs.aws.amazon.com](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/install-sam-cli.html) | `sam --version` |
| AWS CLI | [docs.aws.amazon.com](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html) | `aws --version` |
| curl / jq | System package manager | `curl --version && jq --version` |
| awscurl (for smoke test) | `pip install awscurl` | `awscurl --version` |

### AWS environment

- **Platform account**: The AWS account where Nexus runs. You need credentials with permissions to deploy CloudFormation stacks, create IAM roles, and manage API Gateway, Lambda, DynamoDB, EventBridge, and SES resources.
- **Domain account(s)**: One or more AWS accounts where domain resources will be provisioned. You need credentials with permissions to deploy CloudFormation stacks and create IAM roles.
- **AWS Organizations**: The platform account must belong to an AWS Organization. Nexus uses AWS RAM to share domain namespace information across accounts. Capture the Organization ARN (e.g., `arn:aws:organizations::123456789012:organization/o-abc123`) — you will need it as a parameter.
- **Email addresses** (optional): For approval and rejection notifications during testing. Both the sender and recipient addresses must be verified in SES sandbox mode. If you skip this, you can still approve domains via the API directly.
- **Service quotas**: Default quotas in the platform account support only **3 domains** (the EventBridge Schema Registry limit is 10 registries per region). Before onboarding more, see [Service Quotas and Capacity Planning](service-quotas.md).

### Decide on your namespace

Nexus is a singleton per organization. Platform resources follow the fixed `nexus-*` pattern (e.g., `nexus-bus`); the domain-account trust role follows `{org}-nexus-trust-role` (org-scoped so it's unique across organizations sharing a domain account); domain resources follow `{org}-{env}-{domain}-*` (the env comes from the `Environments` parameter, applied per-domain at provisioning time). Choose a value for:

| Parameter | Example | Notes |
|---|---|---|
| `Organization` | `acme` | Short identifier for your organization |

This value is immutable once deployed. Stack names are fixed (`nexus-core`, `nexus-notifications`, `nexus-trust`) — one deployment per AWS Organization.

---

## Phase 1 — Deploy the notifications stack (optional)

This stack creates SES email identities for approval and rejection notifications. It is for testing against the SES sandbox only. Skip this phase if you do not need email notifications.

**Important:** Deploy this stack in the same AWS region as the Nexus core stack. SES identities are regional.

```bash
sam deploy \
  --config-file samconfig-notifications.toml \
  --stack-name nexus-notifications \
  --parameter-overrides \
    NotificationSenderEmail=<sender@example.com> \
    PlatformOperatorEmail=<platform-team@example.com> \
  --region <region>
```

Example:

```bash
sam deploy \
  --config-file samconfig-notifications.toml \
  --stack-name nexus-notifications \
  --parameter-overrides \
    NotificationSenderEmail=nexus-sender@example.com \
    PlatformOperatorEmail=platform-team@example.com \
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
  --stack-name nexus-notifications \
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
sam deploy --config-file samconfig-core.toml --guided
```

For repeatable deployments using the defaults in `samconfig-core.toml`:

```bash
sam deploy --config-file samconfig-core.toml
```

**Required parameters:**

| Parameter | Description | Example |
|---|---|---|
| `Organization` | Organization identifier used in Nexus resource naming | `acme` |
| `AwsOrganizationArn` | AWS Organization ARN (for RAM sharing of domain namespaces). Distinct from `Organization`. | `arn:aws:organizations::123456789012:organization/o-abc123` |

**Optional parameters:**

| Parameter | Description | Default |
|---|---|---|
| `NotificationSenderEmail` | SES sender address used as the From on all Nexus notification emails (from Phase 1) | `""` (disabled) |
| `PlatformOperatorEmail` | SES recipient that receives domain approval requests — the platform team inbox (from Phase 1) | `""` (disabled) |
| `NamespacePattern` | Resource naming convention | `{org}-{env}-{domain}` |
| `Environments` | Comma-delimited list of environments every approved domain is provisioned into. Platform-team controlled; domain requesters cannot override. Each entry must match `^[a-z0-9][a-z0-9-]{0,31}$`. | `dev,staging,prod` |

> **On `Environments`**: this is an org-wide governance setting. Every domain registered and approved on this Nexus deployment is provisioned across the full list. To add or remove an environment, update the parameter and redeploy — the change applies to *future* registrations only; existing domains keep the environment list they were registered with.

If you completed Phase 1, pass the email addresses:

```bash
sam deploy --config-file samconfig-core.toml \
  --parameter-overrides \
    Organization=acme \
    AwsOrganizationArn=arn:aws:organizations::123456789012:organization/o-abc123 \
    NotificationSenderEmail=nexus-sender@example.com \
    PlatformOperatorEmail=platform-team@example.com
```

### Capture stack outputs

After deployment succeeds, capture the outputs needed for Phase 3:

```bash
aws cloudformation describe-stacks \
  --stack-name nexus-core \
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

## Phase 3 — Onboard a domain account

Deploy the trust stack in each domain account that will own domains. This grants Nexus cross-account access to provision domain resources.

Switch your AWS CLI credentials to the domain account, then:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name nexus-trust \
  --parameter-overrides \
    NexusRoleArn=<NexusFunctionRoleArn from Phase 2> \
    ProvisioningWorkflowRoleArn=<ProvisioningWorkflowRoleArn from Phase 2> \
    Organization=<org> \
  --tags Application=nexus Component=trust \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

Example:

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

A single trust role (`<org>-nexus-trust-role`) serves all environments of all
domains in the account.

For detailed instructions including verification and troubleshooting, see
[trust-stack-deployment.md](trust-stack-deployment.md).

---

## Phase 4 — Smoke test

Verify your installation by running the automated smoke test, which registers a
test domain, approves it, and polls until it reaches ACTIVE status:

```bash
make smoke AWS_REGION=<region> APP_ACCOUNT_ID=<domain-account-id> OWNER_EMAIL=<owner@example.com> SECONDARY_OWNER_EMAIL=<secondary@example.com>
```

Example:

```bash
make smoke AWS_REGION=us-east-1 APP_ACCOUNT_ID=222233334444 OWNER_EMAIL=owner@example.com SECONDARY_OWNER_EMAIL=secondary@example.com
```

The smoke test:

1. Fetches the API endpoint from the core stack outputs
2. Registers a domain named `smoke-test` via `awscurl` (IAM-signed request)
3. Waits for the approval workflow to publish the task token, then approves the domain via the API
4. Polls until the domain reaches `ACTIVE` status (up to ~3.5 minutes)

If all steps pass, your Nexus installation is working. To verify provisioned
resources, describe the domain and check the `resourceArns` map:

```bash
awscurl -X GET "${API_URL}/domains/smoke-test" --region <region> | jq .
```

The response should include an `environments` list (e.g., `["dev","staging","prod"]`)
and a `resourceArns` map keyed by environment with event bus, schema registry,
and DLQ ARNs for each.

The smoke test does not delete the domain. To clean up afterward, run:

```bash
make teardown-domain DOMAIN_NAME=smoke-test
```

### Manual smoke test

If you prefer to run the steps manually (e.g., to test with a real domain name or inspect intermediate states), see the API endpoints below. The API uses IAM authentication — use `awscurl` for signed requests.

```bash
API_URL="<ApiEndpoint from Phase 2>"
REGION="<region>"

# Register
awscurl -X POST "${API_URL}/domains" --region "${REGION}" \
  -H "Content-Type: application/json" \
  -d '{"domainName":"billing","ownerAccountId":"<domain-account-id>","ownerEmail":"<email>","secondaryOwnerEmails":["<secondary-email>"],"description":"Billing domain"}'

# Approve
awscurl -X POST "${API_URL}/domains/registrations/decide" --region "${REGION}" \
  -H "Content-Type: application/json" \
  -d '{"taskToken":"<taskToken>","decision":"ACCEPT","decidedBy":"<email>"}'

# Check status
awscurl -X GET "${API_URL}/domains/<domainName>" --region "${REGION}"
```

---

## Troubleshooting

### SES sandbox: verification emails not received

In SES sandbox mode, both the sender and recipient email addresses must be verified. After deploying the notifications stack, AWS sends a verification email to each address. If you don't receive it:

- Check spam/junk folders.
- Confirm the email addresses are correct in the stack parameters.
- Verify status with `aws ses get-identity-verification-attributes`.
- SES sandbox only allows sending to verified addresses. You cannot send to arbitrary recipients until you request production access.
- The `ownerEmail` and `secondaryOwnerEmails` values submitted during domain registration are notification recipients. In SES sandbox mode, each must be a verified SES identity before any approval, rejection, registration-failure, or provisioning-failure notification can be delivered to them. Either verify each owner email as an SES identity, or request SES production access to lift the sandbox restriction.

### RAM share failed: not in an AWS Organization

The `AwsOrganizationArn` parameter is required. Nexus uses AWS RAM to share domain namespace information across accounts. If you see errors during provisioning related to RAM:

- Confirm the platform account belongs to an AWS Organization.
- Verify the Organization ARN is correct (`arn:aws:organizations::...`).
- Ensure the domain account is in the same Organization.

### Trust role AssumeRole denied

If domain provisioning fails with `AccessDenied` when assuming the trust role:

- Verify the trust stack is deployed in the target domain account.
- Check the trust role's trust policy lists the correct platform account role ARNs.
- Confirm the role ARNs match the `NexusFunctionRoleArn` and `ProvisioningWorkflowRoleArn` outputs from the core stack.

```bash
aws iam get-role \
  --role-name <org>-nexus-trust-role \
  --region <region>
```

### zig 0.16+ `ar` regression breaks SAM build

If `make build` fails with errors like:

```text
ar: error: unable to open '...libring_core_0_17_14_.a': No such file or directory
```

or

```text
ar: error: unable to open '...libaws_lc_0_39_1_crypto.a': No such file or directory
```

This is a regression in zig 0.16.0 where the `ar` subcommand (LLVM Archiver) cannot create new archive files. The `c` (create) modifier is broken — it always attempts to open an existing file first, failing when the output `.a` file does not yet exist. This affects any Rust crate using `cc-rs` for native compilation during cross-builds (including `ring` and `aws-lc-sys`, both used by the AWS SDK).

**Fix:** Downgrade zig to 0.14.x:

```bash
brew unlink zig
brew install zig@0.14
brew link zig@0.14
```

Then clear caches and rebuild:

```bash
rm -rf ~/Library/Caches/cargo-zigbuild
make clean
make build
```

If builds also hang during the `aws-lc-sys` compilation step, set the following environment variable (already included in the Makefile):

```bash
export AWS_LC_SYS_NO_JITTER_ENTROPY=1
```

### SAM build cache is stale

If builds fail or produce unexpected results after changing code:

```bash
make clean    # removes cargo build artifacts and .aws-sam
make build    # fresh build
```

The `sam build -c` command uses a cache (`-c` flag). If you encounter build issues, clearing the cache with `make clean` is the first step.

### DynamoDB table retained after stack delete

The `DomainsTable` has `DeletionPolicy: Retain`. When you delete the core stack, the DynamoDB table is preserved. To fully remove it:

```bash
aws dynamodb delete-table \
  --table-name nexus-domains \
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

# Tear down each domain (drives the deprovision-env workflow per environment)
make teardown-domain DOMAIN_NAME=<domainName>
```

Repeat for each domain. The teardown script drives the same deprovisioning
workflow used by the provisioning saga's rollback path, removing all per-env
resources in both the platform and domain accounts before deleting the domain
record.

### 2. Delete the core stack

```bash
aws cloudformation delete-stack \
  --stack-name nexus-core \
  --region <region>
```

### 3. Delete trust stacks (in each domain account)

Switch credentials to each domain account:

```bash
aws cloudformation delete-stack \
  --stack-name nexus-trust \
  --region <region>
```

### 4. Delete the notifications stack

```bash
aws cloudformation delete-stack \
  --stack-name nexus-notifications \
  --region <region>
```

### 5. Clean up retained resources

The following resources are not automatically deleted when their stacks are removed:

- **DynamoDB table**: `nexus-domains` (retained by `DeletionPolicy`)
- **CloudWatch log groups**: Under `/aws/lambda/nexus-*` and `/aws/apigateway/nexus-*`

```bash
# Delete the retained DynamoDB table
aws dynamodb delete-table --table-name nexus-domains --region <region>

# List and delete log groups
aws logs describe-log-groups \
  --log-group-name-prefix "/aws/lambda/nexus-" \
  --region <region> \
  --query "logGroups[*].logGroupName" --output text

aws logs delete-log-group --log-group-name <log-group-name> --region <region>
```
