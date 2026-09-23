# Install Konductor

[Documentation home](../README.md)

Deploy the platform, onboard an account, and verify a domain. Run these steps only with authorization for the target AWS accounts.

## Overview

Konductor deploys as three AWS CloudFormation stacks across two types of AWS accounts: the platform account and domain accounts.

The platform uses Amazon API Gateway, AWS Lambda, Amazon DynamoDB, AWS Step Functions, and Amazon EventBridge. AWS Identity and Access Management (IAM) roles control access between accounts. Dead-letter queues (DLQs) retain messages after retries fail. The following diagram shows the stack boundaries.

```text
┌─────────────────────────────────────────────────┐
│  Platform Account                                │
│                                                  │
│  1. notifications-stack (optional)               │
│     SES email identities for testing             │
│                                                  │
│  2. konductor core stack (required)                  │
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
│     IAM role granting Konductor cross-account access  │
│                                                  │
└─────────────────────────────────────────────────┘
```

## Prerequisites

### Local toolchain

| Tool | Install | Verify |
|---|---|---|
| Rust stable | [rustup.rs](https://rustup.rs/)—the project pins stable via `rust-toolchain.toml` | `rustc --version` |
| cargo-lambda | [cargo-lambda.info](https://www.cargo-lambda.info/) | `cargo lambda --version` |
| zig 0.14.x (for cross-compilation) | `brew install zig@0.14` or install via [ziglang.org](https://ziglang.org/). **zig 0.16+ has a broken `ar` command**—see [Troubleshooting](installation-troubleshooting.md#zig-016-ar-regression-breaks-sam-build) | `zig version` |
| AWS Serverless Application Model Command Line Interface (AWS SAM CLI) | [Installing the AWS SAM CLI](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/install-sam-cli.html) | `sam --version` |
| AWS Command Line Interface (AWS CLI) | [Installing the AWS CLI](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html) | `aws --version` |
| curl / jq | System package manager | `curl --version && jq --version` |
| awscurl (for smoke test) | `pip install awscurl` | `awscurl --version` |

### AWS environment

- **Platform account**: The AWS account where Konductor runs. You need credentials with permissions to deploy CloudFormation stacks, create IAM roles, and manage API Gateway, Lambda, DynamoDB, EventBridge, and SES resources.
- **Domain accounts**: One or more AWS accounts where domain resources will be provisioned. You need credentials with permissions to deploy CloudFormation stacks and create IAM roles.
- **AWS Organizations**: The platform account must belong to an AWS organization. Konductor uses AWS Resource Access Manager (AWS RAM) to share domain namespace information across accounts. Record the organization's Amazon Resource Name (ARN), for example `arn:aws:organizations::123456789012:organization/o-abc123`, for the deployment parameter.
- **Email addresses** (optional): Amazon Simple Email Service (Amazon SES) sends approval and rejection notifications during testing. Verify both sender and recipient addresses in the Amazon SES sandbox. Without email notifications, approve domains through the API.
- **Service quotas**: Default quotas in the platform account support only **3 domains** (the EventBridge Schema Registry limit is 10 registries per Region). Before onboarding more, see [Service Quotas and Capacity Planning](service-quotas.md).

### Decide on your namespace

Konductor is a singleton per organization. Platform resources follow the fixed `konductor-*` pattern (for example, `konductor-bus`); the domain-account trust role follows `{org}-konductor-trust-role` (org-scoped so it's unique across organizations sharing a domain account); domain resources follow `{org}-{env}-{domain}-*` (the env comes from the `Environments` parameter, applied per-domain at provisioning time). Choose a value for:

| Parameter | Example | Notes |
|---|---|---|
| `Organization` | `acme` | Short identifier for your organization |

This value is immutable once deployed. Stack names are fixed (`konductor-core`, `konductor-notifications`, `konductor-trust`)—one deployment per AWS organization.

---

## Phase 1 — deploy the notifications stack (optional)

This stack creates SES email identities for approval and rejection notifications. It is for testing against the SES sandbox only. Skip this phase if you do not need email notifications.

**Important:** Deploy this stack in the same AWS Region as the Konductor core stack. SES identities are Regional.

```bash
sam deploy \
  --config-file samconfig-notifications.toml \
  --stack-name konductor-notifications \
  --parameter-overrides \
    NotificationSenderEmail=<sender@example.com> \
    PlatformOperatorEmail=<platform-team@example.com> \
  --region <region>
```

Example:

```bash
sam deploy \
  --config-file samconfig-notifications.toml \
  --stack-name konductor-notifications \
  --parameter-overrides \
    NotificationSenderEmail=konductor-sender@example.com \
    PlatformOperatorEmail=platform-team@example.com \
  --region us-east-1
```

### Verify SES identities

SES sandbox requires both email addresses to be verified before you can send:

1. After deploying, check your inbox for SES verification emails from AWS.
2. Choose the verification link in each email.
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
  --stack-name konductor-notifications \
  --query "Stacks[0].Outputs" \
  --region <region>
```

---

## Phase 2 — deploy the Konductor core stack

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

#### Required parameters

Supply the following parameters for the core stack.

| Parameter | Description | Example |
|---|---|---|
| `Organization` | Organization identifier used in Konductor resource naming | `acme` |
| `AwsOrganizationArn` | AWS organization ARN (for RAM sharing of domain namespaces). Distinct from `Organization`. | `arn:aws:organizations::123456789012:organization/o-abc123` |

#### Optional parameters

Use these parameters to configure notifications and domain environments.

| Parameter | Description | Default |
|---|---|---|
| `NotificationSenderEmail` | SES sender address used as the From on all Konductor notification emails (from Phase 1) | `""` (disabled) |
| `PlatformOperatorEmail` | SES recipient that receives domain approval requests—the platform team inbox (from Phase 1) | `""` (disabled) |
| `NamespacePattern` | Resource naming convention | `{org}-{env}-{domain}` |
| `Environments` | Comma-delimited list of environments every approved domain is provisioned into. Platform-team controlled; domain requesters cannot override. Each entry must match `^[a-z0-9][a-z0-9-]{0,31}$`. | `dev,staging,prod` |

> **On `Environments`**: this is an org-wide governance setting. Every domain registered and approved on this Konductor deployment is provisioned across the full list. To add or remove an environment, update the parameter and redeploy — the change applies to *future* registrations only; existing domains keep the environment list they were registered with.

If you completed Phase 1, pass the email addresses:

```bash
sam deploy --config-file samconfig-core.toml \
  --parameter-overrides \
    Organization=acme \
    AwsOrganizationArn=arn:aws:organizations::123456789012:organization/o-abc123 \
    NotificationSenderEmail=konductor-sender@example.com \
    PlatformOperatorEmail=platform-team@example.com
```

### Confirm the platform notifications subscription

When notifications are enabled, the core stack subscribes `PlatformOperatorEmail` to an Amazon Simple Notification Service (Amazon SNS) topic, `konductor-platform-notifications`. The topic forwards lifecycle events from the Konductor bus to platform operators. Confirm the email subscription once:

1. After deploying, check the platform team inbox for an AWS Notification "Subscription Confirmation" email.
2. Choose **Confirm subscription**. Amazon SNS does not deliver operator notifications until you confirm.

### Capture stack outputs

After deployment succeeds, capture the outputs needed for Phase 3:

```bash
aws cloudformation describe-stacks \
  --stack-name konductor-core \
  --query "Stacks[0].Outputs" \
  --region <region>
```

Record the following values:

| Output | Used in phase 3 as |
|---|---|
| `KonductorFunctionRoleArn` | `KonductorRoleArn` parameter |
| `ProvisioningWorkflowRoleArn` | `ProvisioningWorkflowRoleArn` parameter |
| `RetagWorkflowRoleArn` | `RetagWorkflowRoleArn` parameter |

Also note the `IamApiEndpoint` and `OAuthApiEndpoint` outputs—the smoke test uses both (registration and status polling go to the IAM endpoint; approval is OAuth-only).

## Phase 3: onboard a domain account

Follow [trust-stack deployment](trust-stack-deployment.md), including its [unresolved role-parameter discrepancy](../reference/documentation-discrepancies.md#d03). Keep the platform stack outputs from Phase 2 available.

Organization guardrails are optional and separately authorized. They are not part of the ordinary installation path. See [guardrail deployment and verification](guardrails.md), including the unresolved account-selector issue, before considering attachment.

## Phase 4 — smoke test

Verify your installation by running the automated smoke test, which registers a
test domain, approves it, and polls until it reaches ACTIVE status. The approval
step is OAuth-only, so `TOKEN` must be a platform-admin access token (see the
[OAuth Authentication Guide](../guides/authentication.md)):

```bash
make smoke AWS_REGION=<region> APP_ACCOUNT_ID=<domain-account-id> OWNER_EMAIL=<owner@example.com> SECONDARY_OWNER_EMAIL=<secondary@example.com> TOKEN=<platform-admin-access-token>
```

Example:

```bash
make smoke AWS_REGION=us-east-1 APP_ACCOUNT_ID=222233334444 OWNER_EMAIL=owner@example.com SECONDARY_OWNER_EMAIL=secondary@example.com TOKEN=eyJraWQ...
```

The smoke test:

1. Fetches the IAM and OAuth API endpoints from the core stack outputs
2. Registers a domain named `smoke-test` via `awscurl` (IAM-signed request)
3. Waits for the approval workflow to publish the task token, then approves the domain via the OAuth API with the `TOKEN` credentials
4. Polls until the domain reaches `ACTIVE` status (up to ~3.5 minutes)

If all steps pass, your Konductor installation is working. To verify provisioned
resources, describe the domain and check the `resourceArns` map:

```bash
awscurl -X GET "${API_URL}/domains/smoke-test" --region <region> | jq .
```

The response should include an `environments` list (for example, `["dev","staging","prod"]`)
and a `resourceArns` map keyed by environment with event bus, schema registry,
and DLQ ARNs for each.

The smoke test does not delete the domain. To clean up afterward, run:

```bash
make teardown-domain DOMAIN_NAME=smoke-test
```

### Manual smoke test

To test with a specific domain name or inspect intermediate states, use the following manual steps. Registration and status polling use the IAM surface through `awscurl` and AWS Signature Version 4 (SigV4). Approval is OAuth-only, so the decision request uses `curl` with a platform-admin bearer token.

```bash
API_URL="<IamApiEndpoint from Phase 2>"
OAUTH_URL="<OAuthApiEndpoint from Phase 2>"
REGION="<region>"
TOKEN="<platform-admin access token>"

# Register (IAM surface)
awscurl -X POST "${API_URL}/domains" --region "${REGION}" \
  -H "Content-Type: application/json" \
  -d '{"domainName":"billing","ownerAccountId":"<domain-account-id>","ownerEmail":"<email>","secondaryOwnerEmails":["<secondary-email>"],"description":"Billing domain"}'

# Approve (OAuth surface). The task token is listed by
# GET ${OAUTH_URL}/domains/registrations/pending and stored on the domain record.
curl -s -X POST "${OAUTH_URL}/domains/registrations/decide" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"taskToken":"<taskToken>","decision":"ACCEPT"}'

# Check status (IAM surface)
awscurl -X GET "${API_URL}/domains/<domainName>" --region "${REGION}"
```

## Troubleshooting and removal

- [Installation troubleshooting](installation-troubleshooting.md)
- [Uninstall](uninstall.md)
- [Service quotas and capacity planning](service-quotas.md)
