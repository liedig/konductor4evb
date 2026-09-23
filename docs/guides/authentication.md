# Authenticate API requests

[Documentation home](../README.md)

Use OAuth access tokens for interactive callers and IAM signing for automation. Ask a platform administrator to [create your user and access](../operations/access-administration.md) first. Find Cognito settings in the [stack outputs](../operations/access-administration.md#stack-outputs).

> **Review pending:** [D02](../reference/documentation-discrepancies.md#d02), [D06](../reference/documentation-discrepancies.md#d06) records conflicting or unverified guidance retained from the previous documentation. This reorganization does not resolve it.

## Obtaining an access token

Konductor uses the OAuth 2.0 **authorization-code** flow. The app client does not have a client secret (public client), so the token exchange does not require one.

### Option A: hosted UI (browser)

Open this URL in a browser, replacing the placeholders:

```text
https://<OAUTH_DOMAIN>/oauth2/authorize?
  response_type=code&
  client_id=<CLIENT_ID>&
  redirect_uri=http://localhost:3000/callback&
  scope=openid+email+profile
```

After login, Cognito redirects to your callback URL with a `code` parameter:

```text
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

### Option B: CLI (admin auth flow)

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

### Option C: hosted UI (direct login, no redirect)

For quick testing, Cognito supports a login URL that directly returns tokens without a server callback. Use `http://localhost` as the redirect URI:

```text
https://<OAUTH_DOMAIN>/login?
  response_type=code&
  client_id=<CLIENT_ID>&
  redirect_uri=http://localhost&
  scope=openid+email+profile
```

After login, the browser redirects to `http://localhost?code=XXXX`. Copy the code and exchange it as shown in Option A.

### Option D: custom login (SRP, no hosted UI)

First-party consoles (for example the Konductor web UI) sign users in with the
Secure Remote Password (SRP) flow instead of redirecting to the hosted UI. The user-pool client allows
`ALLOW_USER_SRP_AUTH` and `ALLOW_REFRESH_TOKEN_AUTH`; raw-password
authentication stays disabled, so credentials never traverse the client.
AWS Amplify Auth (`signIn`/`fetchAuthSession`) implements the SRP
handshake for browser apps; store tokens in `sessionStorage`, not
`localStorage`.

## Calling the API

Send the access token in the `Authorization` header. Use the [OpenAPI contracts](../reference/api.md) for route definitions and [access-token claims](../reference/authentication.md) for identity requirements.

### Using curl

To list domains, register a domain, or approve a registration, use the following request examples:

```bash
API_URL="https://<API_ID>.execute-api.<REGION>.amazonaws.com/dev"
TOKEN="<ACCESS_TOKEN>"

# List domains
curl -s -H "Authorization: Bearer $TOKEN" "$API_URL/domains" | jq .

# Register a domain (owningTeam must be one of your team groups; platform
# groups are never team groups, so platform teams cannot own domains)
curl -s -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "domainName": "billing",
    "ownerAccountId": "111122223333",
    "ownerEmail": "owner@example.com",
    "secondaryOwnerEmails": ["backup@example.com"],
    "owningTeam": "team-payments"
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

For GUI clients, see [Insomnia and Postman setup](api-clients.md).

## Decoding and inspecting a token

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

## Troubleshooting

Use the response status and error message to identify the next action.

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `401 Unauthorized` | Missing or expired token, wrong token type (ID token), or invalid signature | Verify `token_use: "access"`, check `exp`, ensure the Authorization header is `Bearer <token>` |
| `403 AccessDenied` | User is in `platform-auditor` but calling a write/approve endpoint | Add user to `platform-admin` group |
| `403` on decide endpoint | Principal is IAM (not OAuth), or missing `platform-admin` role | The decide endpoint is OAuth-only; use an access token from a `platform-admin` user |
| `400` with `decidedBy` error | Request body contains a `decidedBy` field | Remove `decidedBy` from the body; it is derived from the token's `sub` claim |
| `400 ValidationException` on decide | Missing `taskToken` in the body, or missing `justification` on a REJECT | Include `taskToken` (from the approval email) and `justification` when rejecting |
| Writes return `RegistrationFailed` / `UpdateDomainFailed` with an `unauthorized` reason | Caller lacks the required role: registration needs `owningTeam` ∈ the caller's team groups; update needs an `owner` binding on the domain or `platform-admin` | Join the owning team group, or ask a platform admin to grant an `owner` binding via `POST /bindings` |
| `403` on list/describe for a domain your team owns | Missing `owner` binding (domains registered before ownership-at-approval) | Backfill with `DOMAIN_TEAMS='{"<domain>": "<team-group>"}' make backfill-owner-bindings` (dry-run with `DRY_RUN=1`) |

## IAM surface (automation)

For automation that uses AWS credentials, use the AWS Identity and Access Management (IAM) API surface. This includes continuous integration and delivery (CI/CD) pipelines. Sign requests with AWS Signature Version 4 (SigV4). You do not need a Cognito token.

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
