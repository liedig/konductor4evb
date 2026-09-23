# Manage users and access

[Documentation home](../README.md)

For platform administrators setting up interactive users, team bindings, and entitlements. Users obtain tokens through the [authentication guide](../guides/authentication.md).

> **Review pending:** [D05](../reference/documentation-discrepancies.md#d05), [D06](../reference/documentation-discrepancies.md#d06) records conflicting or unverified guidance retained from the previous documentation. This reorganization does not resolve it.

## What Konductor provisions

When you deploy the core stack (`make deploy-core`), AWS Serverless Application Model (AWS SAM) creates the following Amazon Cognito resources.

| Resource | Purpose |
|----------|---------|
| **User pool** (`{org}-konductor-users`) | Stores user accounts. Email is the username attribute. Administrators create accounts; self-registration is unavailable. Passwords require at least 12 characters, mixed case, and numbers. |
| **User pool domain** (`{org}-konductor-{account-id}`) | Hosted user interface (UI) domain for the OAuth authorization-code flow. |
| **App client** (`{org}-konductor-control-plane`) | Public OAuth client using the authorization-code flow with `openid`, `email`, and `profile` scopes. It has no client secret. Access token validity is configurable (default 60 minutes). |
| **Group: `platform-admin`** | Maps to the `PlatformAdmin` global role. Can access all API methods including POST (writes) and the approval endpoint. |
| **Group: `platform-auditor`** | Maps to the `PlatformAuditor` global role. Can only access GET (read) endpoints. Cannot mutate or approve. |

### Stack outputs

After deployment, AWS CloudFormation exposes the following outputs:

```text
CognitoUserPoolId        – e.g. ap-southeast-2_abc123def
CognitoUserPoolIssuer    – https://cognito-idp.<region>.amazonaws.com/<pool-id>
CognitoUserPoolClientId  – e.g. 1a2b3c4d5e6f7g8h9i0j
CognitoJwksUri           – https://cognito-idp.<region>.amazonaws.com/<pool-id>/.well-known/jwks.json
CognitoOAuthDomain       – https://<domain>.auth.<region>.amazoncognito.com
OAuthApiEndpoint          – https://<api-id>.execute-api.<region>.amazonaws.com/dev
```

Retrieve them with:

```bash
aws cloudformation describe-stacks \
  --stack-name konductor-core \
  --query 'Stacks[0].Outputs[?starts_with(OutputKey, `Cognito`) || OutputKey==`OAuthApiEndpoint`].[OutputKey,OutputValue]' \
  --output table
```

## Setting up users

Use the development seed script or create users and assign groups manually.

### 0. Shortcut: seed the development personas

`make seed-users` (or `./scripts/seed-users.sh`) creates the two standard
test personas with permanent passwords, creating the team group if absent:

```bash
ALICE_PASSWORD='...' BOB_PASSWORD='...' make seed-users
```

- `alice@example.com` → `platform-admin` (approvals inbox, enable/disable, full reads)
- `bob@example.com` → `team-payments` (team caller: registers and owns domains, scoped reads)

The script is idempotent—rerunning re-asserts passwords and membership.
The following sections describe the equivalent manual flow.

### 1. Create a user

```bash
aws cognito-idp admin-create-user \
  --user-pool-id <POOL_ID> \
  --username alice@example.com \
  --user-attributes Name=email,Value=alice@example.com \
  --temporary-password "TempPass123!"
```

The user receives a welcome email with a temporary password. They must set a permanent password on first login.

### 2. Assign a group

```bash
# For a platform administrator (full access including approvals):
aws cognito-idp admin-add-user-to-group \
  --user-pool-id <POOL_ID> \
  --username alice@example.com \
  --group-name platform-admin

# For a read-only auditor:
aws cognito-idp admin-add-user-to-group \
  --user-pool-id <POOL_ID> \
  --username bob@example.com \
  --group-name platform-auditor
```

A user can belong to both groups. If `platform-admin` is present, the admin role takes precedence and the user has full access.

### 3. (Optional) set a permanent password without email

For testing or automation setups where email delivery is not configured:

```bash
aws cognito-idp admin-set-user-password \
  --user-pool-id <POOL_ID> \
  --username alice@example.com \
  --password "SecurePass123!" \
  --permanent
```

## Bootstrapping role bindings

Resource authorization decisions are answered by the grant engine from the `konductor-authorization-bindings` table: a principal (a Cognito team group, issuer-qualified, or an AWS account) holds a role (`publisher` or `subscriber`) on a domain. Until a binding exists, every resource-scoped action by that principal is denied.

Binding administration cannot deadlock against an empty table: the `platform-admin` global role resolves from the controlled Cognito group (`platform-admin`), never from the grant table. Any platform-admin access token can always create the first bindings.

### Seeding developer bindings

Use the seed helper instead of hand-writing curl calls. It goes through the real admin API (same validation, conditional writes, and audit records as production) and is safe to rerun—identical creates are idempotent:

```bash
TOKEN="<platform-admin access token>" \
BINDINGS='[
  {"domain":"orders","role":"publisher","principal":{"type":"aws-account","accountId":"111122223333"}},
  {"domain":"orders","role":"publisher","principal":{"type":"cognito-group","issuer":"https://cognito-idp.<region>.amazonaws.com/<pool-id>","group":"orders-publishers"}}
]' \
make seed-bindings
```

`API_URL` defaults to the stack's `OAuthApiEndpoint` output (set `AWS_REGION`); override with `API_URL=...` when targeting a non-default endpoint.

When creating or revoking bindings, apply these rules:

- The Cognito group value must be qualified with the exact issuer URL of the user pool (see the `CognitoUserPoolIssuer` stack output). A binding for `orders-publishers` on a different issuer never matches tokens from this pool.
- AWS-account bindings apply to every principal in the account (IAM role, user, or session); there is no per-ARN granularity.
- Server-assigned fields (`bindingId`, `state`, `createdAt`, `createdBy`) are rejected if supplied in a create body.
- Revocation is a durable delete that takes effect on the next authorization decision; list bindings with `GET /bindings?domain=<domain>` to find the `bindingId` to revoke.

### Seeding entitlements (migration path)

Consumer domains retain event access through **entitlements**. Seed them the same way—through the admin API, idempotent on rerun:

```bash
TOKEN="<platform-admin access token>" \
ENTITLEMENTS='[
  {"consumerDomain":"checkout","producerDomain":"orders","scopePattern":"*"},
  {"consumerDomain":"billing","producerDomain":"orders","scopePattern":"Order*"}
]' \
make seed-entitlements
```

Migration order matters: seed the `subscriber` **bindings first** (`make seed-bindings`), then the entitlements, then let the revocation worker and hourly sweep re-evaluate any existing subscriptions—subscriptions whose coverage lapsed are disabled, never deleted, and nothing re-enables them automatically. The scope grammar: `*` covers the producer's whole domain including future schemas, an exact event name covers all its versions including future ones, and a trailing `*` covers a name family.

## Related topics

- [API contracts](../reference/api.md)
- [Identity and grant engine](../architecture/authorization.md)
- [Entitlement revocation](../architecture/entitlements.md)
