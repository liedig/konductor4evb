# Konductor for Amazon EventBridge Installation Guide

Konductor for Amazon EventBridge manages event domains and cross-account routing. This guide covers deployment in your AWS environment and verification through domain registration, approval, and activation.

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
| zig 0.14.x (for cross-compilation) | `brew install zig@0.14` or install via [ziglang.org](https://ziglang.org/). **zig 0.16+ has a broken `ar` command**—see [Troubleshooting](#zig-016-ar-regression-breaks-sam-build) | `zig version` |
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

---

## Phase 3 — onboard a domain account

Deploy the trust stack in each domain account that will own domains. This grants Konductor cross-account access to provision domain resources, including tagging them with the domain's tag set. **Existing installations**: redeploy the trust stack after upgrading past the resource-tagging change—without the added tag permissions, app-side provisioning fails at the tagging step. Likewise, after upgrading past the editable cost-center change, redeploy the trust stack with the new `RetagWorkflowRoleArn` parameter—without it, cost-center retagging of app-side resources fails with an assume-role denial and the platform emits `RetagDomainFailed`.

Switch your AWS CLI credentials to the domain account, then:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name konductor-trust \
  --parameter-overrides \
    KonductorRoleArn=<KonductorFunctionRoleArn from Phase 2> \
    ProvisioningWorkflowRoleArn=<ProvisioningWorkflowRoleArn from Phase 2> \
    RetagWorkflowRoleArn=<RetagWorkflowRoleArn from Phase 2> \
    Organization=<org> \
  --tags Application=konductor Component=trust \
  --capabilities CAPABILITY_NAMED_IAM \
  --region <region>
```

Example:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name konductor-trust \
  --parameter-overrides \
    KonductorRoleArn=arn:aws:iam::111111111111:role/konductor-function-role \
    ProvisioningWorkflowRoleArn=arn:aws:iam::111111111111:role/konductor-provisioning-workflow-role \
    RetagWorkflowRoleArn=arn:aws:iam::111111111111:role/konductor-retag-workflow-role \
    Organization=acme \
  --tags Application=konductor Component=trust \
  --capabilities CAPABILITY_NAMED_IAM \
  --region us-east-1
```

A single trust role (`<org>-konductor-trust-role`) serves all environments of all
domains in the account.

For detailed instructions including verification and troubleshooting, see
[Trust Stack Deployment Guide](trust-stack-deployment.md).

---

## Phase 3.5 — organization guardrails (optional, separately authorized)

> **Warning: Separate authorization required.** This phase changes AWS
> Organizations policy attachments. It requires organization-admin
> credentials, explicit authorization from your cloud governance process,
> and completed [sandbox verification](#verification--local-fixtures-first-then-authorized-sandbox) before any production
> attachment. Skipping this phase leaves Konductor fully functional but
> without the deny ceiling that stops domain-account administrators from
> modifying Konductor-managed resources. The trust stack alone grants
> Konductor access but constrains no one else.

The trust stack (Phase 3) is an *allow grant*: it lets Konductor manage
`{org}-*` resources in the domain account. It does not prevent a
domain-account administrator from deleting or reconfiguring those
resources. The guardrail stack (`guardrail-stack.yaml`) closes that gap
with three service control policies (SCPs)—a **deny ceiling** over every
principal in the covered accounts. SCPs restrict; they never grant. Team
roles still need their ordinary IAM allows, and Konductor still needs the
trust role.

### Deployment order

Deploy this phase after the trust stack and after meeting the
[prerequisites](#prerequisites-1). This is a separate deployment step, not part
of `make deploy-*`.

### Required inputs

| Input | Description |
|---|---|
| `Organization` | The same short identifier used in Phases 2–3 (for example, `acme`) |
| `KonductorTrustRoleArn` | Exact ARN of the trust role from Phase 3 (for example, `arn:aws:iam::<domain-account>:role/acme-konductor-trust-role`). Exact match—no wildcards. |
| `RecoveryRoleArn` | Exact ARN of a centrally controlled recovery/maintenance role. This role is exempt from every SCP deny and is the only in-model maintenance path for the trust role. |
| `TargetOuIds` and/or `TargetAccountIds` | Organizational units (OUs), individual account IDs, or both for SCP attachment. Specify at least one target. |

### Prerequisites

1. **Recovery role bootstrapped and tested first.** Create the recovery
   role in the target accounts, confirm its central principal can assume
   it, and confirm it can read the trust role—**before** attaching any
   SCP. Attaching guardrails without a working recovery path risks lockout
   (the ultimate fallback is central SCP detachment by the organization
   administrator).
2. **Resource inventory.** Confirm existing `{org}-*` buses, queues, and
   roles in the target accounts match the supported naming selectors.
   Resources outside the selector patterns are not covered.

### Deploy (organization admin credentials)

```bash
aws cloudformation deploy \
  --template-file guardrail-stack.yaml \
  --stack-name konductor-guardrails \
  --parameter-overrides \
    Organization=acme \
    KonductorTrustRoleArn=arn:aws:iam::<domain-account-id>:role/acme-konductor-trust-role \
    RecoveryRoleArn=arn:aws:iam::<domain-account-id>:role/<recovery-role-name> \
    TargetAccountIds=<sandbox-account-id> \
  --region <region>
```

### Verification — local fixtures first, then authorized sandbox

1. **Local verification**: run the repository's policy/template fixture
   tests. These prove the SCP statements match the enforcement matrix —
   they do **not** prove live enforcement.
2. **Authorized sandbox verification**: attach the SCPs to an isolated
   sandbox account/OU first and run the full acceptance procedure in
   `openspec/changes/protect-domain-managed-resources/sandbox-acceptance.md`
   (positive team operations, denied administrator operations, Konductor
   provisioning/compensation, recovery path). Retain the recorded
   evidence: policy versions, targets, principals, observed AWS outcomes.
3. **Staged production attachment**: only after a documented sandbox pass,
   attach to production OUs/accounts in stages, with explicit
   organization-admin approval at each stage.

> **Do not claim protection without sandbox verification.** A deployed
> stack and passing fixtures show the policies exist and are well-formed;
> only the sandbox run demonstrates that AWS actually denies the protected
> operations. Until then, describe the installation as "guardrails
> deployed, enforcement unverified."

### Rollback

If a guardrail breaks a legitimate operation: **detach** the affected SCP
from the affected target → **repair** via the recovery role (fix policy
content or exception ARNs) → **re-attach** the validated version. Do not
delete protected resources as a rollback strategy. Record the temporary
protection gap while detached.

### Coverage limits (read before claiming protection)

- SCPs do **not** apply to the AWS Organizations **management account** —
  principals operating there bypass every statement.
- **Service-linked roles** are exempt from SCP evaluation for actions they
  are designed to perform.
- Organization administrators remain trusted and retain full control of
  attachments.
- Console links in the web UI are navigation conveniences, not an
  authorization mechanism—they do not enforce read-only (or any) AWS
  access.

---

## Phase 4 — smoke test

Verify your installation by running the automated smoke test, which registers a
test domain, approves it, and polls until it reaches ACTIVE status. The approval
step is OAuth-only, so `TOKEN` must be a platform-admin access token (see the
[OAuth Authentication Guide](development/oauth-authentication-guide.md)):

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

---

## Troubleshooting

### SES sandbox: verification emails not received

In SES sandbox mode, both the sender and recipient email addresses must be verified. After deploying the notifications stack, AWS sends a verification email to each address. If you don't receive it:

- Check spam/junk folders.
- Confirm the email addresses are correct in the stack parameters.
- Verify status with `aws ses get-identity-verification-attributes`.
- SES sandbox only allows sending to verified addresses. You cannot send to arbitrary recipients until you request production access.
- The `ownerEmail` and `secondaryOwnerEmails` values submitted during domain registration are notification recipients. In SES sandbox mode, each must be a verified SES identity before any approval, rejection, registration-failure, or provisioning-failure notification can be delivered to them. Either verify each owner email as an SES identity, or request SES production access to lift the sandbox restriction.

### RAM share failed: not in an AWS organization

The `AwsOrganizationArn` parameter is required. Konductor uses AWS RAM to share domain namespace information across accounts. If you see errors during provisioning related to RAM:

- Confirm the platform account belongs to an AWS organization.
- Verify the Organization ARN is correct (`arn:aws:organizations::...`).
- Ensure the domain account is in the same Organization.

### Trust role AssumeRole denied

If domain provisioning fails with `AccessDenied` when assuming the trust role:

- Verify the trust stack is deployed in the target domain account.
- Check the trust role's trust policy lists the correct platform account role ARNs.
- Confirm the role ARNs match the `KonductorFunctionRoleArn`, `ProvisioningWorkflowRoleArn`, and `RetagWorkflowRoleArn` outputs from the core stack.

```bash
aws iam get-role \
  --role-name <org>-konductor-trust-role \
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

This is a regression in zig 0.16.0 where the `ar` subcommand (LLVM Archiver) cannot create new archive files. The `c` (create) modifier is broken—it always attempts to open an existing file first, failing when the output `.a` file does not yet exist. This affects any Rust crate using `cc-rs` for native compilation during cross-builds (including `ring` and `aws-lc-sys`, both used by the AWS SDK).

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
  --table-name konductor-domains \
  --region <region>
```

---

## Uninstall

To remove a Konductor installation completely:

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
  --stack-name konductor-core \
  --region <region>
```

### 3. Delete trust stacks (in each domain account)

Switch credentials to each domain account:

```bash
aws cloudformation delete-stack \
  --stack-name konductor-trust \
  --region <region>
```

### 4. Delete the notifications stack

```bash
aws cloudformation delete-stack \
  --stack-name konductor-notifications \
  --region <region>
```

### 5. Clean up retained resources

The following resources are not automatically deleted when their stacks are removed:

- **DynamoDB table**: `konductor-domains` (retained by `DeletionPolicy`)
- **CloudWatch log groups**: Under `/aws/lambda/konductor-*` and `/aws/apigateway/konductor-*`

```bash
# Delete the retained DynamoDB table
aws dynamodb delete-table --table-name konductor-domains --region <region>

# List and delete log groups
aws logs describe-log-groups \
  --log-group-name-prefix "/aws/lambda/konductor-" \
  --region <region> \
  --query "logGroups[*].logGroupName" --output text

aws logs delete-log-group --log-group-name <log-group-name> --region <region>
```
