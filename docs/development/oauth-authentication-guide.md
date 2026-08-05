# OAuth Authentication Guide

This guide explains how the Cognito user pool is provisioned, what claims the Nexus authorizer validates, and how to obtain an access token and call the OAuth API surface using API testing tools such as Insomnia, Postman, or curl.

---

## What Nexus Provisions

When you deploy the core stack (`make deploy-core`), SAM creates the following Cognito resources automatically:

| Resource | Purpose |
|----------|---------|
| **User Pool** (`{org}-nexus-users`) | Stores user accounts. Email is the username attribute. Admin-created users only (no self-registration). Password policy: 12+ chars, mixed case, numbers. |
| **User Pool Domain** (`{org}-nexus-{account-id}`) | Hosted UI domain for the OAuth authorization-code flow. |
| **App Client** (`{org}-nexus-control-plane`) | OAuth client using the authorization-code flow with `openid`, `email`, and `profile` scopes. No client secret (public client). Access token validity is configurable (default 60 minutes). |
| **Group: `platform-admin`** | Maps to the `PlatformAdmin` global role. Can access all API methods including POST (writes) and the approval endpoint. |
| **Group: `platform-auditor`** | Maps to the `PlatformAuditor` global role. Can only access GET (read) endpoints. Cannot mutate or approve. |

### Stack Outputs

After deployment, the following CloudFormation outputs are available:

```
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
  --stack-name nexus-core \
  --query 'Stacks[0].Outputs[?starts_with(OutputKey, `Cognito`) || OutputKey==`OAuthApiEndpoint`].[OutputKey,OutputValue]' \
  --output table
```

---

## Setting Up Users

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

### 3. (Optional) Set a permanent password without email

For testing or automation setups where email delivery is not configured:

```bash
aws cognito-idp admin-set-user-password \
  --user-pool-id <POOL_ID> \
  --username alice@example.com \
  --password "SecurePass123!" \
  --permanent
```

---

## Required Access-Token Claims

The Nexus OAuth authorizer validates the following claims on every request. An access token that fails any check is rejected with `401 Unauthorized`.

| Claim | Required Value | Verified By |
|-------|---------------|-------------|
| `sub` | Any non-empty string (Cognito subject) | Used as `principalId` for audit logging and `decidedBy` in approvals |
| `iss` | Must match the configured user-pool issuer | JWT signature validation (`jsonwebtoken` crate) |
| `exp` | Must be in the future | JWT standard validation |
| `token_use` | Must be `"access"` | Authorizer handler (ID tokens are rejected) |
| `client_id` | Must match the configured app client ID | Authorizer handler |
| `cognito:groups` | Optional, but determines role-based access | Authorizer maps known groups to roles; unknown groups are preserved as opaque team-group identifiers |
| `kid` (header) | Must match a key in the user pool's JWKS | JWKS lookup with cache and refresh on unknown `kid` |

### Group-to-Role Mapping

| Cognito Group | Global Role | API Access |
|---------------|------------|------------|
| `platform-admin` | `PlatformAdmin` | All methods (GET, POST, PUT, PATCH, DELETE) on all OAuth endpoints |
| `platform-auditor` | `PlatformAuditor` | GET-only on read endpoints; all write and approve endpoints return `403` |
| Any other group (e.g. `team-payments`) | No global role (opaque team-group) | No API access by itself; team-group identifiers are carried in the trusted-principal context for the future authorization engine |

### What the Authorizer Does NOT Do

- It does not read the request body or query parameters for identity.
- It does not log token material or sensitive claims.
- It does not cache authorizer results by default (`AuthorizerResultTtlInSeconds: 0`).
- It does not assign permissions from team-group names; only the two controlled groups above map to roles.

---

## Obtaining an Access Token

Nexus uses the OAuth 2.0 **authorization-code** flow. The app client does not have a client secret (public client), so the token exchange does not require one.

### Option A: Hosted UI (Browser)

Open this URL in a browser, replacing the placeholders:

```
https://<OAUTH_DOMAIN>/oauth2/authorize?
  response_type=code&
  client_id=<CLIENT_ID>&
  redirect_uri=http://localhost:3000/callback&
  scope=openid+email+profile
```

After login, Cognito redirects to your callback URL with a `code` parameter:

```
http://localhost:3000/callback?code=XXXX
```

Exchange the code for tokens:

```bash
curl -X POST "https://<OAUTH_DOMAIN>/oauth2/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "grant_type=authorization_code" \
  -d "client_id=<CLIENT_ID>" \
  -d "code=XXXX" \
  -d "redirect_uri=http://localhost:3000/callback"
```

The response contains `access_token`, `id_token`, and `refresh_token`. Use the **`access_token`** (not the ID token) in API calls.

### Option B: CLI (Admin Auth Flow)

For testing without a browser, use the admin authentication flow:

```bash
aws cognito-idp admin-initiate-auth \
  --user-pool-id <POOL_ID> \
  --client-id <CLIENT_ID> \
  --auth-flow ADMIN_USER_PASSWORD_AUTH \
  --auth-parameters USERNAME=alice@example.com,PASSWORD='<YOUR_PASSWORD>'
```

If the response contains `ChallengeName: NEW_PASSWORD_REQUIRED`, respond:

```bash
aws cognito-idp admin-respond-to-auth-challenge \
  --user-pool-id <POOL_ID> \
  --client-id <CLIENT_ID> \
  --challenge-name NEW_PASSWORD_REQUIRED \
  --session <SESSION_FROM_PREVIOUS_RESPONSE> \
  --challenge-responses NEW_PASSWORD='<YOUR_NEW_PASSWORD>',USERNAME=alice@example.com
```

The `AuthenticationResult.AccessToken` field is the token you need.

### Option C: Hosted UI (Direct Login, No Redirect)

For quick testing, Cognito supports a login URL that directly returns tokens without a server callback. Use `http://localhost` as the redirect URI:

```
https://<OAUTH_DOMAIN>/login?
  response_type=code&
  client_id=<CLIENT_ID>&
  redirect_uri=http://localhost&
  scope=openid+email+profile
```

After login, the browser redirects to `http://localhost?code=XXXX`. Copy the code and exchange it as shown in Option A.

---

## Calling the API

### Available OAuth Endpoints

| Method | Path | Required Role | Description |
|--------|------|--------------|-------------|
| GET | `/domains` | admin or auditor | List domains (paginated, filterable) |
| POST | `/domains` | admin | Register a new domain (async, 202) |
| GET | `/domains/{domainName}` | admin or auditor | Describe a single domain |
| PUT | `/domains/{domainName}` | admin | Update domain metadata (async, 202) |
| POST | `/domains/registrations/decide` | admin | Approve or reject a registration |
| GET | `/subscriptions` | admin or auditor | List subscriptions (filtered by caller account) |
| POST | `/subscriptions` | admin | Subscribe to a producer schema (async, 202) |
| GET | `/subscriptions/{subscriptionId}` | admin or auditor | Describe a subscription |
| PATCH | `/subscriptions/{subscriptionId}` | admin | Enable/disable per environment (async, 202) |
| DELETE | `/subscriptions/{subscriptionId}` | admin | Unsubscribe (async, 202) |
| GET | `/schemas` | admin or auditor | List schemas for a domain |
| GET | `/schemas/{schemaName}` | admin or auditor | Describe a schema (latest versions per env) |
| POST | `/bindings` | admin | Create a role binding (idempotent) |
| GET | `/bindings?domain=…` | admin | List role bindings for a domain |
| DELETE | `/bindings/{bindingId}` | admin | Revoke a role binding (durable) |

### Using curl

```bash
API_URL="https://<API_ID>.execute-api.<REGION>.amazonaws.com/dev"
TOKEN="<ACCESS_TOKEN>"

# List domains
curl -s -H "Authorization: Bearer $TOKEN" "$API_URL/domains" | jq .

# Register a domain
curl -s -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "domainName": "billing",
    "ownerAccountId": "111122223333",
    "ownerEmail": "owner@example.com",
    "secondaryOwnerEmails": ["backup@example.com"]
  }' \
  "$API_URL/domains" | jq .

# Approve a registration (decidedBy is derived from the token, not the body)
curl -s -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "taskToken": "TASK_TOKEN_FROM_APPROVAL_EMAIL",
    "decision": "ACCEPT"
  }' \
  "$API_URL/domains/registrations/decide" | jq .
```

### Using Insomnia

1. **Create a new environment** (or use the base environment) with these variables:

   ```json
   {
     "apiUrl": "https://<API_ID>.execute-api.<REGION>.amazonaws.com/dev",
     "accessToken": "PASTE_YOUR_ACCESS_TOKEN_HERE"
   }
   ```

2. **Add a Bearer token to all requests**: Create a sub-environment or folder-level auth:
   - Type: `Bearer Token`
   - Token: `{{ accessToken }}`

3. **Create requests** using the variable:
   - `GET {{ apiUrl }}/domains`
   - `POST {{ apiUrl }}/domains` with a JSON body
   - `POST {{ apiUrl }}/domains/registrations/decide` with a JSON body

4. **(Optional) Automate token retrieval**: Insomnia supports pre-request scripts. You can add a script that calls the Cognito token endpoint and stores the result:

   ```javascript
   // In the request "Before Send Script" tab:
   const response = await insomnia.sendRequest({
     url: 'https://<OAUTH_DOMAIN>/oauth2/token',
     method: 'POST',
     headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
     body: {
       grant_type: 'authorization_code',
       client_id: '<CLIENT_ID>',
       code: pm.environment.get('authCode'),
       redirect_uri: 'http://localhost'
     }
   });
   const body = JSON.parse(response.body);
   insomnia.set('accessToken', body.access_token);
   ```

   For the admin auth flow (no browser), use a pre-request script that calls `aws cognito-idp admin-initiate-auth` via a shell command, or set the token manually after retrieving it with the AWS CLI.

### Using Postman

1. **Create an environment** with `apiUrl` and `accessToken` variables.

2. **Set up authorization at the collection level**:
   - Type: `Bearer Token`
   - Token: `{{accessToken}}`

3. **Import the OpenAPI contract**:
   - File > Import > `api/openapi-oauth.yaml`
   - Postman generates all endpoints with the correct paths and methods.

4. **(Optional) OAuth 2.0 flow automation**:
   - Collection > Authorization > Type: `OAuth 2.0`
   - Configure:
     - Callback URL: `http://localhost`
     - Auth URL: `https://<OAUTH_DOMAIN>/oauth2/authorize`
     - Access Token URL: `https://<OAUTH_DOMAIN>/oauth2/token`
     - Client ID: `<CLIENT_ID>`
     - Scope: `openid email profile`
     - Grant Type: `Authorization Code`
   - Click "Get New Access Token" to open the browser, log in, and exchange the code automatically.

---

## Bootstrapping Role Bindings

Resource authorization decisions are answered by the grant engine from the `nexus-authorization-bindings` table: a principal (a Cognito team group, issuer-qualified, or an AWS account) holds a role (`publisher` or `subscriber`) on a domain. Until a binding exists, every resource-scoped action by that principal is denied.

Binding administration cannot deadlock against an empty table: the `platform-admin` global role resolves from the controlled Cognito group (`platform-admin`), never from the grant table. Any platform-admin access token can always create the first bindings.

### Seeding developer bindings

Use the seed helper instead of hand-writing curl calls. It goes through the real admin API (same validation, conditional writes, and audit records as production) and is safe to re-run — identical creates are idempotent:

```bash
TOKEN="<platform-admin access token>" \
BINDINGS='[
  {"domain":"orders","role":"publisher","principal":{"type":"aws-account","accountId":"111122223333"}},
  {"domain":"orders","role":"publisher","principal":{"type":"cognito-group","issuer":"https://cognito-idp.<region>.amazonaws.com/<pool-id>","group":"orders-publishers"}}
]' \
make seed-bindings
```

`API_URL` defaults to the stack's `OAuthApiEndpoint` output (set `AWS_REGION`); override with `API_URL=...` when targeting a non-default endpoint.

Notes:

- The Cognito group value must be qualified with the exact issuer URL of the user pool (see the `CognitoUserPoolIssuer` stack output). A binding for `orders-publishers` on a different issuer never matches tokens from this pool.
- AWS-account bindings apply to every principal in the account (IAM role, user, or session); there is no per-ARN granularity.
- Server-assigned fields (`bindingId`, `state`, `createdAt`, `createdBy`) are rejected if supplied in a create body.
- Revocation is a durable delete that takes effect on the next authorization decision; list bindings with `GET /bindings?domain=<domain>` to find the `bindingId` to revoke.

## Decoding and Inspecting a Token

To verify your token has the correct claims before making API calls:

```bash
# Decode the JWT header and payload (no signature verification, for inspection only):
echo "<ACCESS_TOKEN>" | cut -d. -f2 | base64 -d 2>/dev/null | jq .
```

Expected output for a `platform-admin` user:

```json
{
  "sub": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "iss": "https://cognito-idp.ap-southeast-2.amazonaws.com/ap-southeast-2_abc123def",
  "client_id": "1a2b3c4d5e6f7g8h9i0j",
  "token_use": "access",
  "exp": 1722240000,
  "cognito:groups": ["platform-admin", "team-payments"],
  "username": "alice@example.com"
}
```

Key things to verify:
- `token_use` is `"access"` (not `"id"`)
- `client_id` matches the `CognitoUserPoolClientId` output
- `iss` matches the `CognitoUserPoolIssuer` output
- `cognito:groups` contains `platform-admin` or `platform-auditor`
- `exp` is in the future

---

## Troubleshooting

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| `401 Unauthorized` | Missing or expired token, wrong token type (ID token), or invalid signature | Verify `token_use: "access"`, check `exp`, ensure the Authorization header is `Bearer <token>` |
| `403 AccessDenied` | User is in `platform-auditor` but calling a write/approve endpoint | Add user to `platform-admin` group |
| `403` on decide endpoint | Principal is IAM (not OAuth), or missing `platform-admin` role | The decide endpoint is OAuth-only; use an access token from a `platform-admin` user |
| `400` with `decidedBy` error | Request body contains a `decidedBy` field | Remove `decidedBy` from the body; it is derived from the token's `sub` claim |
| `400 ValidationException` on decide | Missing `taskToken` in the body, or missing `justification` on a REJECT | Include `taskToken` (from the approval email) and `justification` when rejecting |
| Token works but writes return `SubscribeFailed` / `RegistrationFailed` | OAuth mutations are not yet enabled on these paths (`IamOnlyAuthorizer` bridge denies OAuth writes) | This is expected until those paths migrate to the grant engine. IAM-authenticated callers (AWS CLI with SigV4) can still write. |

---

## IAM Surface (Automation)

For AWS-native automation (CI/CD, scripts using AWS credentials), use the IAM API surface instead. No Cognito token is needed; requests are signed with SigV4.

```bash
IAM_API_URL="https://<IAM_API_ID>.execute-api.<REGION>.amazonaws.com/dev"

# List domains (IAM-signed automatically by the AWS CLI)
aws apigateway test-invoke-method \
  --rest-api-id <IAM_API_ID> \
  --resource-id <RESOURCE_ID> \
  --http-method GET \
  --path-with-query-string "/domains"
```

Or use `--profile` with the AWS SDK in your automation code. The IAM surface uses `AWS_IAM` authorization and identifies callers by their AWS account ID. The approval (decide) endpoint is not available on the IAM surface.
