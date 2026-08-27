# Delta: control-plane-access

## ADDED Requirements

### Requirement: Direct SRP authentication for custom login clients

The Cognito user-pool client SHALL allow `ALLOW_USER_SRP_AUTH` and `ALLOW_REFRESH_TOKEN_AUTH` in its explicit auth flows, so first-party interactive clients (e.g. the Konductor web UI) can authenticate users with a custom login experience without the hosted UI. Tokens obtained through SRP SHALL be indistinguishable to the OAuth authorizer from tokens obtained via the hosted UI: the same claims, group mappings, and trusted-principal normalization SHALL apply. Self-service sign-up SHALL remain disabled; users remain admin-provisioned.

#### Scenario: Custom login obtains usable tokens

- **WHEN** a user signs in through a first-party client using SRP against the user pool
- **THEN** Cognito SHALL issue access, ID, and refresh tokens carrying the user's group memberships
- **AND THEN** the OAuth control-plane API SHALL accept the access token and resolve the same trusted principal as a hosted-UI login

#### Scenario: Refresh without re-authentication

- **WHEN** a first-party client presents a valid refresh token
- **THEN** Cognito SHALL issue a new access token within the configured validity bounds
- **AND THEN** the user's group memberships SHALL be re-evaluated from current pool state

#### Scenario: No self-registration

- **WHEN** an unprovisioned identity attempts to sign up through any enabled flow
- **THEN** Cognito SHALL reject the attempt
- **AND THEN** only admin-created users SHALL be able to authenticate
