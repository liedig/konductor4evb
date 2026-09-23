# Access-token claims

[Documentation home](../README.md)

The token contract consumed by the OAuth authorizer. For obtaining and sending a token, see [authentication](../guides/authentication.md).

## Required access-token claims

The Konductor OAuth authorizer validates the following JSON Web Token (JWT) claims on every request. It checks signatures against the user pool's JSON Web Key Set (JWKS). If a required check fails, the authorizer returns `401 Unauthorized`.

| Claim | Required value | Verified by |
|-------|---------------|-------------|
| `sub` | Any non-empty string (Cognito subject) | Used as `principalId` for audit logging and `decidedBy` in approvals |
| `iss` | Must match the configured user-pool issuer | JWT signature validation (`jsonwebtoken` crate) |
| `exp` | Must be in the future | JWT standard validation |
| `token_use` | Must be `"access"` | Authorizer handler (ID tokens are rejected) |
| `client_id` | Must match the configured app client ID | Authorizer handler |
| `cognito:groups` | Optional, but determines role-based access | Authorizer maps known groups to roles; unknown groups are preserved as opaque team-group identifiers |
| `kid` (header) | Must match a key in the user pool's JWKS | JWKS lookup with cache and refresh on unknown `kid` |

### Group-to-role mapping

The authorizer maps controlled Amazon Cognito groups to global roles.

| Cognito group | Global role | API access |
|---------------|------------|------------|
| `platform-admin` | `PlatformAdmin` | All methods on all OAuth endpoints |
| `platform-auditor` | `PlatformAuditor` | GET-only on read endpoints; all write and approve endpoints return `403` |
| Any other group (for example, `team-payments`) | No global role (opaque team-group) | No API access by itself; team-group identifiers are carried in the trusted-principal context for grant-engine role-binding evaluation |

### What the authorizer does not do

The authorizer has the following boundaries:

- It does not read the request body or query parameters for identity.
- It does not log token material or sensitive claims.
- It does not cache authorizer results by default (`AuthorizerResultTtlInSeconds: 0`).
- It does not assign permissions from team-group names. Only `platform-admin` and `platform-auditor` map to global roles.
