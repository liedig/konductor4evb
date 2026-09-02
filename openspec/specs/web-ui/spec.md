# Capability: Web UI

## Purpose

The Konductor web console: a browser-based control-plane UI where platform admins review and decide domain registrations and domain teams request domains, manage owners, discover schemas, and manage subscriptions and access grants.

## Requirements

### Requirement: Custom Cognito login and session

The web console SHALL authenticate users through a Konductor-branded login page using Cognito SRP against the existing user pool (not the hosted UI). Sessions SHALL persist per browser tab (sessionStorage) with refresh-token renewal; logging out SHALL clear the session and return to `/login`. Unauthenticated navigation to any protected route SHALL redirect to `/login`. The console SHALL read its runtime configuration (API base URL, Cognito pool/client IDs) from a served `config.json` so stack changes never require a rebuild.

#### Scenario: Successful sign-in lands on the persona dashboard

- **WHEN** a provisioned user submits valid credentials on the login page
- **THEN** the console SHALL authenticate via SRP and store the session per tab
- **AND THEN** SHALL redirect platform-role users to the admin inbox and all other users to their domains dashboard

#### Scenario: Failed sign-in shows a safe error

- **WHEN** a user submits invalid credentials
- **THEN** the console SHALL show a generic "invalid credentials" error without disclosing which factor failed
- **AND THEN** SHALL NOT retain any token material

#### Scenario: Expired access token renews transparently

- **WHEN** an API call receives a 401 and the session holds a valid refresh token
- **THEN** the console SHALL refresh the access token and retry the call once
- **AND THEN** if refresh fails SHALL clear the session and redirect to `/login`

### Requirement: Role-gated navigation and persona dashboards

Navigation SHALL reflect the caller's groups: platform roles see the admin area; users with team groups see the domain-team area; users holding both see both. Route guards SHALL enforce area membership client-side as navigation hygiene (the API remains the enforcing boundary). The platform dashboard SHALL be inbox-first: pending domain approvals are the landing content. The domain-team dashboard SHALL be domain-first: the caller's owned domains with status.

#### Scenario: Platform admin lands on the approvals inbox

- **WHEN** a platform-admin signs in
- **THEN** the landing view SHALL list pending domain registrations with requester, owning team, and age
- **AND THEN** an empty inbox SHALL show a deliberate empty state

#### Scenario: Domain owner lands on their domains

- **WHEN** a user whose teams own domains signs in
- **THEN** the landing view SHALL list those domains with status badges
- **AND THEN** a user owning no domains SHALL see an empty state guiding them to request a domain

#### Scenario: Area routes reject the wrong persona

- **WHEN** a non-platform user navigates to an `/admin/*` route
- **THEN** the console SHALL redirect to their own dashboard
- **AND THEN** any API call the route would have made SHALL remain denied server-side

### Requirement: Domain request and owner management

The console SHALL provide a domain-request form collecting domain name, owning team (picked from the caller's team groups; pre-selected when exactly one), owner account ID, owner email, at least one secondary owner email, optional description and an optional cost center. Client validation SHALL mirror the API contract (name pattern, 12-digit account, email formats, min one secondary owner, cost center non-empty when supplied and at most 256 characters). Owners SHALL be able to edit description, owner email, secondary owner emails, and cost center from the domain detail page; all other attributes SHALL render read-only. Platform admins SHALL be able to disable and re-enable domains from the admin domain list.

#### Scenario: Request submits as an async command

- **WHEN** an owner submits a valid request
- **THEN** the console SHALL POST the registration (including `owningTeam`) and show the accepted command with its request ID
- **AND THEN** the new domain SHALL appear in the caller's list as pending

#### Scenario: Owner edits only the mutable fields

- **WHEN** an owner saves changes from the domain detail edit form
- **THEN** the console SHALL submit only description, emails, and cost center
- **AND THEN** name, account, and environments SHALL have no editable affordance

#### Scenario: Cost center edit is pre-filled and validated

- **WHEN** an owner opens the domain detail edit form
- **THEN** the cost center field SHALL be pre-filled with the current value
- **AND THEN** an empty cost center SHALL be rejected client-side as the API contract requires

#### Scenario: Platform admin disables a domain

- **WHEN** a platform admin chooses disable on an ACTIVE domain and confirms
- **THEN** the console SHALL call the state endpoint and reflect INACTIVE
- **AND THEN** non-admin users SHALL never see the affordance

### Requirement: Approvals inbox

Platform admins SHALL review pending registrations from a dedicated inbox showing request details (name, description, owning team, owner account and emails, age). Approve SHALL record the decision immediately; reject SHALL require a justification. Decided items SHALL leave the inbox on success, and failures (expired token, conflict) SHALL surface a descriptive error.

#### Scenario: Approve from the inbox

- **WHEN** a platform admin approves a pending registration
- **THEN** the console SHALL submit the decision with the registration's task token
- **AND THEN** the item SHALL be removed from the inbox

#### Scenario: Rejection requires justification

- **WHEN** a platform admin attempts to reject without entering a justification
- **THEN** the console SHALL block submission and mark the justification field as required

### Requirement: Catalog discovery and subscription journeys

The console SHALL offer the entitlement-scoped catalog with producer and environment filters, schema detail (per-environment latest versions, JSON content), and a subscribe action that confirms the consumer domain and reports activation progress by polling until ENABLED or FAILED (surfacing the failure reason). "My subscriptions" SHALL list inbound subscriptions with pause/resume/unsubscribe; the domain detail page SHALL show outbound subscriptions (who consumes this domain's schemas). Producers SHALL see their own domain's schemas on the domain page without needing an entitlement.

#### Scenario: Subscribe confirms and tracks activation

- **WHEN** a team member subscribes their domain to a covered schema
- **THEN** the console SHALL submit the command, acknowledge acceptance (202), and poll the subscription
- **AND THEN** SHALL settle on ENABLED, on FAILED with the machine-readable reason, or on "still enabling" after the polling window

#### Scenario: Uncovered schema offers no subscribe action

- **WHEN** a schema is not covered for any of the caller's consumer domains
- **THEN** the schema detail SHALL explain that access requires an entitlement
- **AND THEN** no subscribe affordance SHALL be offered

#### Scenario: Pause and resume

- **WHEN** a team member pauses an ENABLED subscription
- **THEN** the console SHALL call the state endpoint and reflect the paused status
- **AND THEN** resuming SHALL re-enable delivery

### Requirement: Schema authoring journeys

The producer's domain page schemas panel SHALL offer a "Register schema" action to callers with a `publisher` binding on that domain. The form SHALL accept an event name and JSON Schema draft-4 content; `schemaType` SHALL be fixed to `JSONSchemaDraft4` and SHALL NOT be selectable. The form SHALL display the target environment, derived from the console's configured API stage, and SHALL NOT offer an environment selector. Each listed schema SHALL offer an "append version" action with the same content editor. Content SHALL be validated as parseable JSON client-side before submission. Success SHALL report the schema name and resulting version and SHALL refresh the panel's per-environment version badges. Failure responses SHALL be surfaced inline — `409 Conflict` for a duplicate schema, `403 Forbidden` for missing authorization, `400 Bad Request` with the server's validation detail — and SHALL NOT clear the form's entered content. Callers without a `publisher` binding SHALL see no authoring affordances.

#### Scenario: Publisher registers a schema

- **WHEN** a publisher submits the register form with an event name and valid draft-4 content
- **THEN** the console SHALL create the schema and display its canonical name and version 1
- **AND THEN** the panel SHALL refresh to show the new schema with its environment badge

#### Scenario: Publisher appends a version

- **WHEN** a publisher submits new content via a schema's append-version action
- **THEN** the console SHALL append the version and display the new version number
- **AND THEN** the panel's version badge for the target environment SHALL update

#### Scenario: Malformed JSON caught before submission

- **WHEN** the content editor holds text that is not parseable JSON
- **THEN** the console SHALL block submission and identify the parse error
- **AND THEN** no API call SHALL be made

#### Scenario: Server validation failure is surfaced and content is retained

- **WHEN** the API rejects a submission with `400 Bad Request` (for example, content that is not a valid draft-4 document)
- **THEN** the console SHALL display the server's validation detail inline
- **AND THEN** the form SHALL retain the entered event name and content

#### Scenario: Duplicate schema surfaces conflict

- **WHEN** the API returns `409 Conflict` because the derived schema name already exists in the stage-selected registry
- **THEN** the console SHALL display the conflict inline
- **AND THEN** the form SHALL retain the entered content

#### Scenario: Target environment is shown, not chosen

- **WHEN** a publisher opens either authoring form
- **THEN** the form SHALL display the target environment derived from the console's API stage
- **AND THEN** no environment input SHALL be offered

#### Scenario: Non-publisher sees no authoring affordances

- **WHEN** a caller without a `publisher` binding on the domain views the domain page
- **THEN** the register and append-version actions SHALL NOT be rendered
- **AND THEN** the read-only schema list behavior SHALL remain unchanged

### Requirement: Access grants on the producer domain page

The producer's domain page SHALL include an access panel listing the domain's entitlements (consumer domain, scope pattern, created metadata) and allowing a publisher (or platform admin) to grant or revoke access. The grant form SHALL select the consumer domain from the names-only directory picker (no free text) and the covered schemas via a schema picker with an explicit wildcard option. Revocation SHALL require confirmation and SHALL explain that active subscriptions will be disabled asynchronously.

#### Scenario: Grant access via picker

- **WHEN** a publisher grants access by choosing a consumer domain from the directory picker and a schema pattern
- **THEN** the console SHALL create the entitlement and show it in the panel
- **AND THEN** the consumer domain name SHALL never be hand-typed

#### Scenario: Revoke warns about live subscribers

- **WHEN** a publisher revokes an entitlement
- **THEN** the console SHALL require confirmation stating that covered active subscriptions will be disabled
- **AND THEN** the entitlement SHALL disappear from the panel on success

### Requirement: Documented visual design system

The product SHALL have a design system documented at `docs/development/design-system.md` covering: design principles (clean, airy, professional; layered depth; generous whitespace; color-with-intent status semantics; `rounded-lg`/`rounded-xl` radius; `shadow-sm`-only elevation), the token set (color, type, spacing), core primitives (buttons, inputs, selects, badges/status pills, cards, tables, modal/confirm, toast, empty state), and page patterns (form, list+filter, detail, inbox). Tokens SHALL be implemented as Tailwind v4 `@theme` CSS variables with dark as the default theme, and every documented token SHALL correspond to a real token in code. A `/design-system` gallery route SHALL render the primitives for reference.

#### Scenario: Dark theme is structural

- **WHEN** the console renders any screen
- **THEN** components SHALL consume semantic tokens and never raw palette utilities
- **AND THEN** the default theme SHALL be dark without any toggle or inversion pass

#### Scenario: Documentation matches implementation

- **WHEN** a token or primitive is documented in design-system.md
- **THEN** it SHALL exist in the codebase at the documented path
- **AND THEN** the gallery route SHALL render each documented primitive

### Requirement: Static hosting and deployment

The console SHALL build to static assets deployed to an S3 bucket behind a CloudFront distribution with TLS, SPA fallback (403/404 → `index.html`), and response security headers including a restrictive content-security-policy. Deployment SHALL write `config.json` from current stack outputs (OAuth API URL, Cognito pool/client IDs) and SHALL register the web origins in the stack's OAuth callback/logout URLs.

#### Scenario: Deploy publishes config from the stack

- **WHEN** an operator runs the web deploy target
- **THEN** the distribution SHALL serve the latest build
- **AND THEN** `config.json` SHALL reflect the current stack outputs without a rebuild

#### Scenario: Deep links survive refresh

- **WHEN** a signed-in user refreshes on a deep route such as `/domains/orders`
- **THEN** CloudFront SHALL serve the SPA
- **AND THEN** the router SHALL restore the route after session revalidation
