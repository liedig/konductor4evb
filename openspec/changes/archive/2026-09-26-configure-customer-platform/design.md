# Design

## Context

See proposal.md — Why. Current state that shapes the approach:

- `konductor-core-stack.yaml` declares every customer setting as a deployment parameter: `Organization`, `Environments` (default `dev,staging,prod`), `NamespacePattern` (default `{org}-{env}-{domain}`), `AwsOrganizationArn`, `NotificationSenderEmail` and `PlatformOperatorEmail` (both-or-neither via the `NotificationsEnabled` condition), `AccessTokenValidityMinutes`, `OAuthCallbackUrls`, `OAuthLogoutUrls`.
- Constraints are thin: `AccessTokenValidityMinutes` has Min/Max bounds; `Organization`, `NamespacePattern`, and per-entry environment syntax have none — the environment regex lives only in a description string.
- `samconfig-core.toml` is committed with real customer values (an AWS Organization ARN and email addresses).
- `platform-admin` / `platform-auditor` are literal group names in both the template (Cognito groups) and `lib/konductor-common/src/principal.rs`; the OAuth authorizer resolves `platform-admin` independently of grant storage, so access administration never deadlocks on an empty bindings table.
- Environment-named API stages are the environment selector; environment names also appear in Lambda environment (`ENVIRONMENTS`) and request validation.
- Resource naming bakes `Organization` into platform resources (`{org}-konductor-*`), domain resources (`{org}-{env}-{domain}-*`), and trust-role names in domain accounts (`<org>-konductor-trust-role`); `AwsOrganizationArn` scopes RAM sharing.

## Goals / Non-Goals

**Goals:**

- An explicit, validated configuration contract covering every customer-supplied setting.
- A preflight check with actionable feedback that runs before any deploy.
- Deploy-time rejection of changes to fixed-at-setup settings on an existing installation.
- Customer values out of version control.
- The no-self-escalation invariant stated as a requirement and pinned by a test.

**Non-Goals:**

- No runtime configuration service, settings API, or console setup page.
- No environment addition or removal migration (deferred to a named future change).
- No customer-named global-role groups.
- No Konductor-specific bootstrap tooling; platform-owner onboarding is native Cognito administration.
- Event metadata composition stays separate (see proposal.md).

## Decisions

### D1: Deploy-time contract; parameters are the single source of truth

Every customer setting remains a CloudFormation deployment input. There is no runtime settings store, so there is no precedence question and no activation semantics. Updates to updateable settings are redeployments with new values.

Alternatives considered: a runtime configuration service (adds the rename/orphan hazards the proposal warns against, with no requirement forcing runtime mutability) and a hybrid precedence model (two truths to reconcile). Both rejected.

### D2: Lifecycle classes

| Class | Settings | Rule |
|---|---|---|
| Fixed at setup | `Organization`, `NamespacePattern`, `Environments`, `AwsOrganizationArn` | Deployment tooling refuses a changed value on an existing installation with an actionable message; changing it means a new deployment. |
| Updateable | `NotificationSenderEmail`, `PlatformOperatorEmail`, `AccessTokenValidityMinutes`, `OAuthCallbackUrls`, `OAuthLogoutUrls` | Change by redeployment with new values; preflight re-validates prerequisites. |

Rationale: the fixed four are baked into resource names, trust-role names in every domain account, RAM share scope, stage names, and Lambda environment. Environment addition or removal on a live platform is a provisioning-shaped migration (add = provision every domain into the new environment; remove = teardown) and is deferred to a named future change (`extend-platform-environments`), keeping this change free of half-specified migration machinery. Most organisations hold a fixed environment set, so the restriction matches the common case.

### D3: Two-layer validation

1. Template constraints for what CloudFormation can express: `AllowedPattern` and length bounds on `Organization`, a whole-list entry pattern on `Environments`, and a placeholder-token charset plus length on `NamespacePattern`; existing Min/Max bounds retained. Placeholder *presence* in `NamespacePattern` is a preflight rule, not a template pattern — a lookahead-based template regex could not be verified without a deployment.
2. A preflight (`make validate-config`) for what CloudFormation cannot express: per-entry environment syntax (`^[a-z0-9][a-z0-9-]{0,31}$`), uniqueness, and count; `AwsOrganizationArn` format with an optional reachability check; SES verification status of both notification addresses (both verified, or both empty meaning notifications explicitly disabled); HTTPS for OAuth URLs except local development addresses.

The preflight also carries the **drift guard**: it reads the deployed stack's parameters and refuses the deploy when a fixed-at-setup value differs, naming the setting and the redeploy path. This turns "must not silently rename, replace, orphan, or delete existing resources" from documentation into a mechanism.

### D4: One customer-facing configuration file, kept out of git

A single `konductor-platform.toml` holds the customer inputs. The committed artifact is `konductor-platform.toml.example`; the real file is gitignored. Preflight and deploy both consume it (preflight validates, then renders SAM parameter overrides), so validation and deploy can never read different values.

`samconfig-core.toml` loses its committed customer values; non-customer deploy settings remain. Existing installations transition by generating the file from the deployed stack's parameters — the deployed stack remains the record until the file exists. This also removes the committed Organization ARN and email addresses from the repository.

### D5: Access administration is Cognito-native; no self-escalation

Global-role group membership is managed by the customer operator through Cognito administrative actions (console or CLI), not through anything Konductor ships. The group names stay fixed literals (`platform-admin`, `platform-auditor`); group configurability is rejected because it touches the template, the authorizer mapping in `principal.rs`, and the documentation while buying nothing that membership management does not already provide.

The invariant, stated as a requirement and pinned by a test: no Konductor route, binding, entitlement, or configuration setting lets a caller create users, modify global-role group membership, or grant platform global roles. Resource-role bindings never expand into global roles. The authorizer's `platform-admin` resolution remains independent of grant storage, so an operator with AWS-level Cognito access can always establish or recover the first platform administrator — break-glass is an AWS-account capability, not a Konductor mechanism.

### D6: Notification settings remain conditionally present

The both-or-neither `NotificationsEnabled` condition stays. Preflight makes the disabled state explicit ("notifications disabled: both addresses empty") instead of letting it pass silently, and fails when exactly one address is set or an address is set but not verified in SES.

## Risks / Trade-offs

- Immutability restrictions may need walking back when a real customer needs environment addition; walking back a documented restriction is cheap compared to supporting silent drift. The future change is named so the door is visibly open.
- Preflight duplicates a few rules the template also expresses; the duplication is accepted because preflight owns the actionable message and the template owns last-line enforcement.
- Gitignoring the real configuration changes the local development flow; `make deploy` targets must read the configuration file, and the example file must stay current with the contract.
- Operators can still bypass preflight with a direct `sam deploy`; the drift guard reduces but cannot eliminate deliberate circumvention. Template constraints are the backstop.
