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

Konductor is a singleton per organization. Platform resources follow the fixed `konductor-*` pattern (for example, `konductor-bus`). The domain-account trust role follows `{org}-konductor-trust-role`, scoped by organization so it stays unique when organizations share a domain account. Domain resources follow `{org}-{env}-{domain}-*`, where `{env}` comes from the `Environments` parameter and is applied per domain at provisioning time. Choose a value for:

| Parameter | Example | Notes |
|---|---|---|
| `Organization` | `acme` | Short identifier for your organization |

This value is immutable once deployed, as are the environments list, namespace pattern, and AWS Organization ARN—see the lifecycle classes in the [configuration contract](../reference/configuration.md). Stack names are fixed (`konductor-core`, `konductor-notifications`, `konductor-trust`)—one deployment per AWS organization.

### Credentials and Region

The installation runs make targets against the platform account. A target uses the same credentials as the AWS CLI: the profile named by `AWS_PROFILE`, credential environment variables, or the default profile. If the platform account is not your default profile, export its profile before you start:

```bash
export AWS_PROFILE=<platform-account-profile>
```

To pin a profile on a single command instead, pass `PLATFORM_PROFILE=<profile>` to a target that accepts it; `make help` lists them. The other targets, including the smoke test, always use the current credentials. The trust stack deployment in Phase 3 takes the domain account's profile as `APP_PROFILE`.

Pass `AWS_REGION=<region>` to each make target that calls AWS, as the commands on this page do. Without it, a target falls back to `AWS_DEFAULT_REGION`, then to the Region configured for `PLATFORM_PROFILE` or the current profile, then to `us-east-1`.

---

## Phase 1 — deploy the notifications stack (optional)

This stack creates SES email identities for approval and rejection notifications. It is for testing against the SES sandbox only. Skip this phase if you do not need email notifications.

**Important:** Deploy this stack in the same AWS Region as the Konductor core stack. SES identities are Regional.

The `deploy-notifications` target runs `sam deploy` with the stack settings in `samconfig-notifications.toml`, which names the stack `konductor-notifications`:

```bash
make deploy-notifications \
  AWS_REGION=<region> \
  NOTIFICATION_SENDER_EMAIL=<sender@example.com> \
  PLATFORM_OPERATOR_EMAIL=<platform-team@example.com>
```

Example:

```bash
make deploy-notifications \
  AWS_REGION=us-east-1 \
  NOTIFICATION_SENDER_EMAIL=konductor-sender@example.com \
  PLATFORM_OPERATOR_EMAIL=platform-team@example.com
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

Both identities should show `VerificationStatus: Success`. `make validate-config` re-checks this status in Phase 2 before a core deploy enables notifications.

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

### Configure the platform

Customer values live in `konductor-platform.toml` at the repository root. The file is gitignored—it is the one place customer values live. Create it from the committed example and fill in your values:

```bash
cp konductor-platform.toml.example konductor-platform.toml
```

Every setting, its syntax, default, and lifecycle class is defined in the [configuration contract](../reference/configuration.md). Four settings are fixed at setup (`organization`, `aws_organization_arn`, `environments`, `namespace_pattern`); the rest can change later by editing the file and redeploying.

Validate before deploying:

```bash
make validate-config AWS_REGION=<region>
```

The preflight checks the contract rules and the SES verification status of both notification addresses. Once a stack exists, it also refuses fixed-at-setup changes against the deployed stack.

### Deploy

```bash
make deploy-core AWS_REGION=<region>
```

This runs the same validation first, then deploys with parameter overrides rendered from `konductor-platform.toml`—deploy and validation cannot read different values.

> **On `environments`**: this is an org-wide governance setting, fixed at setup. Every domain registered and approved on this Konductor deployment is provisioned across the full list. The preflight refuses a changed list against a deployed stack; adding or removing environments on a live platform is a provisioning-shaped migration deferred to a future change.
>
> The list also drives the API stages and the console's `/api/{environment}` routes. The first entry is the authoring environment, and every later entry is a promotion target. `make build` (`scripts/validate-config.py --render-stages`) renders the promotion-target stages into the marked block in `konductor-core-stack.yaml`. Deploys where the first entry is not `dev` rewrite the SAM-managed stage to match it. Because the stage resources keep stable logical IDs, adopting the rendered stages on an existing installation is an in-place update with no stage replacement. The phantom `Stage` stage that older deployments carry comes from SAM's generated `Deployment` resource and is inert.

### Existing installations: adopt the configuration file

Installations deployed before this flow have their values in `samconfig-core.toml` parameter overrides or deploy history. Generate the file from the deployed stack instead of retyping values:

```bash
python3 scripts/validate-config.py --init-from-stack --region <region> > konductor-platform.toml
make validate-config AWS_REGION=<region>
```

Review the generated file—settings absent from the deployed stack's parameters fall back to the contract defaults. Once validation passes, `make deploy-core` deploys with exactly the validated values. The committed `samconfig-core.toml` no longer carries parameter overrides.

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

Follow [trust-stack deployment](trust-stack-deployment.md) for each domain account. It needs the three role ARNs from the Phase 2 outputs above, so keep them available.

Organization guardrails are optional and separately authorized. They are not part of the ordinary installation path. See [guardrail deployment and verification](guardrails.md), including the recorded sandbox acceptance evidence, before considering attachment.

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
make teardown-domain AWS_REGION=<region> DOMAIN_NAME=smoke-test
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
