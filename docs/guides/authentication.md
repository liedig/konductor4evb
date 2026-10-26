# Authenticate API requests

[Documentation home](../README.md)

Use OAuth access tokens for interactive callers and IAM signing for automation. Ask a platform administrator to [create your user and access](../operations/access-administration.md) first. Find Cognito settings in the [stack outputs](../operations/access-administration.md#stack-outputs).

## Obtaining an access token

Konductor's user-pool app client is a public client: it has no client secret, so no step needs one. It supports two ways to sign in:

- The OAuth 2.0 **authorization-code** flow through the Cognito hosted UI (Option A).
- **Secure Remote Password (SRP)** sign-in from your own code (Options B and C).

Flows that pass the password through the client, such as `USER_PASSWORD_AUTH` and `ADMIN_USER_PASSWORD_AUTH`, are disabled.

The examples use these placeholders:

| Placeholder | Value |
|---|---|
| `<OAUTH_DOMAIN>` | The `CognitoOAuthDomain` stack output without its `https://` prefix |
| `<CLIENT_ID>` | The `CognitoUserPoolClientId` stack output |
| `<POOL_ID>` | The `CognitoUserPoolId` stack output |

### Option A: hosted UI (browser)

Open this URL in a browser, replacing the placeholders:

```text
https://<OAUTH_DOMAIN>/oauth2/authorize?response_type=code&client_id=<CLIENT_ID>&redirect_uri=http://localhost:3000/callback&scope=openid+email+profile
```

The `redirect_uri` must exactly match one of the platform's configured callback URLs (`oauth_callback_urls` in the [platform configuration](../operations/configure-platform.md#settings-you-can-change-later)). The default is `http://localhost:3000/callback`; Cognito rejects any other value with a `redirect_mismatch` error. To use a different address, a platform administrator adds it to `oauth_callback_urls` and redeploys the core stack.

If your account still has a temporary password, the hosted UI asks you to choose a new one. After sign-in, Cognito redirects the browser to the callback URL with a `code` parameter:

```text
http://localhost:3000/callback?code=XXXX
```

Nothing needs to listen on that address. If the page does not load, copy the `code` value from the browser's address bar. The code works once and expires after five minutes.

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

### Option B: SRP from a script (no browser)

Without a browser, sign in with an SRP client library. The AWS CLI cannot compute the SRP handshake on its own. This example uses [pycognito](https://github.com/NabuCasa/pycognito), a third-party Python library:

```bash
python3 -m pip install pycognito
```

Save this as `konductor-token.py`:

```python
import getpass

from pycognito import Cognito

user = Cognito("<POOL_ID>", "<CLIENT_ID>", username="alice@example.com")
user.authenticate(password=getpass.getpass())
print(user.access_token)
```

Then capture the token:

```bash
TOKEN=$(python3 konductor-token.py)
```

SRP sign-in calls the Cognito user-pool API directly and needs no AWS credentials. If your account still has a temporary password, `authenticate` raises `ForceChangePasswordException`. Sign in once through the hosted UI (Option A) to choose a new password, or ask an administrator to [set a permanent password](../operations/access-administration.md#3-optional-set-a-permanent-password-without-email).

### Option C: SRP in your own application

First-party consoles (for example the Konductor web console) sign users in with SRP instead of redirecting to the hosted UI. AWS Amplify Auth (`signIn`/`fetchAuthSession`) implements the SRP handshake for browser apps; store tokens in `sessionStorage`, not `localStorage`.

### Refreshing an access token

Access tokens expire after the platform's `access_token_validity_minutes` (60 minutes by default). A refresh token from the hosted UI stays valid for 30 days; exchange it for a new access token without signing in again:

```bash
curl -X POST "https://<OAUTH_DOMAIN>/oauth2/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "grant_type=refresh_token" \
  -d "client_id=<CLIENT_ID>" \
  -d "refresh_token=<REFRESH_TOKEN>"
```

The response carries a new `access_token` and `id_token`; keep using the same refresh token. With Option B, run the script again.

## Calling the API

Send the access token in the `Authorization` header. Use the [OpenAPI contracts](../reference/api.md) for route definitions and [access-token claims](../reference/authentication.md) for identity requirements.

### Using curl

To list domains, register a domain, or approve a registration, use the following request examples:

```bash
API_URL="https://<API_ID>.execute-api.<REGION>.amazonaws.com/dev"  # OAuthApiEndpoint stack output
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

# Approve a registration (platform-admin only). The task token comes from the
# pending-registrations inbox; decidedBy is derived from your access token.
TASK_TOKEN=$(curl -s -H "Authorization: Bearer $TOKEN" \
  "$API_URL/domains/registrations/pending" |
  jq -r '.registrations[] | select(.domainName == "billing") | .taskToken')

curl -s -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d "$(jq -n --arg token "$TASK_TOKEN" '{taskToken: $token, decision: "ACCEPT"}')" \
  "$API_URL/domains/registrations/decide" | jq .
```

To reject, send `"decision": "REJECT"` with a `justification`. The **Approvals** page in the Konductor console makes the same calls.

For GUI clients, see [Insomnia and Postman setup](api-clients.md).

## Decoding and inspecting a token

To verify your token has the correct claims before making API calls:

```bash
# Decode the JWT payload (no signature verification, for inspection only).
# The payload is unpadded base64url, so jq converts it before decoding.
echo "<ACCESS_TOKEN>" | jq -R 'split(".")[1] | gsub("-";"+") | gsub("_";"/") | @base64d | fromjson'
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
- `cognito:groups` lists your team groups (for example, `team-payments`), plus `platform-admin` or `platform-auditor` for platform staff
- `exp` is in the future

## Troubleshooting

Use the response status and error message to identify the next action.

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| `InvalidParameterException` saying the flow is not enabled for this client, from `initiate-auth` or `admin-initiate-auth` | Password-based flows are disabled on the app client | Use the hosted UI (Option A) or SRP (Option B) |
| `redirect_mismatch` from the hosted UI | The `redirect_uri` is not one of the configured callback URLs | Use a configured URL (default `http://localhost:3000/callback`), or ask a platform administrator to add yours to `oauth_callback_urls` |
| `401 Unauthorized` | Missing or expired token, wrong token type (ID token), or invalid signature | Verify `token_use: "access"`, check `exp`, ensure the Authorization header is `Bearer <token>` |
| `403 AccessDenied` | User is in `platform-auditor` but calling a write/approve endpoint | Add user to `platform-admin` group |
| `403` on decide endpoint | Principal is IAM (not OAuth), or missing `platform-admin` role | The decide endpoint is OAuth-only; use an access token from a `platform-admin` user |
| `400` with `decidedBy` error | Request body contains a `decidedBy` field | Remove `decidedBy` from the body; it is derived from the token's `sub` claim |
| `400 ValidationException` on decide | Missing `taskToken` in the body, or missing `justification` on a REJECT | Include the `taskToken` from `GET /domains/registrations/pending`, and a `justification` when rejecting |
| `409 ConflictException` on decide | The registration was already decided, or its 7-day approval window expired | Reload the pending inbox; an expired request is already `REJECTED` with `decidedBy: system:timeout` |
| Writes return `RegisterDomainFailed` / `UpdateDomainFailed` with an `unauthorized` reason | Caller lacks the required role: registration needs `owningTeam` ∈ the caller's team groups; update needs an `owner` binding on the domain or `platform-admin` | Join the owning team group, or ask a platform admin to grant an `owner` binding via `POST /bindings` |
| `404` on describe, or missing from `GET /domains`, for a domain your team owns | Your team has no `owner` binding on the domain. Team callers see a domain only through that binding, apart from their own pending or rejected registrations in the list. Domains registered through the IAM surface, or before approval created owner bindings, have none. A new registration gets its binding at approval; see [what happens next](domains.md#what-happens-next). | Ask a platform administrator to grant your team an `owner` binding with `POST /bindings`. For older domains, they can run `DOMAIN_TEAMS='{"<domain>": "<team-group>"}' make backfill-owner-bindings` with a platform-admin `TOKEN` (preview with `DRY_RUN=1`). |

## IAM surface (automation)

For automation that uses AWS credentials, use the AWS Identity and Access Management (IAM) API surface. This includes continuous integration and delivery (CI/CD) pipelines. Sign requests with AWS Signature Version 4 (SigV4) for the `execute-api` service. You do not need a Cognito token.

```bash
IAM_API_URL="https://<IAM_API_ID>.execute-api.<REGION>.amazonaws.com/dev"  # IamApiEndpoint stack output

# List domains, signed with your current AWS credentials
awscurl --service execute-api --region <REGION> "$IAM_API_URL/domains"
```

In automation code, sign requests with your AWS SDK's SigV4 signer. The IAM surface uses `AWS_IAM` authorization and identifies callers by their AWS account ID. Platform administration, including the approval (decide) endpoint and the pending-registrations inbox, is not available on the IAM surface, and its register request does not accept `owningTeam` (see [domain fields](domains.md#fields)).
