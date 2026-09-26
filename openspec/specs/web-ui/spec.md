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

The console SHALL provide a domain-request form collecting domain name, owning team (picked from the caller's team groups; pre-selected when exactly one), owner account ID, owner email, at least one secondary owner email, optional description and an optional cost center. The form SHALL also offer one optional account-override field per platform-configured environment (read from the runtime configuration's environment list): an empty field means the owner account hosts that environment, and a filled field pins that environment to the given account. Client validation SHALL mirror the API contract (name pattern, 12-digit account, email formats, min one secondary owner, cost center non-empty when supplied and at most 256 characters, each account override either empty or a 12-digit account ID). The submitted request SHALL carry only the overridden environments as a sparse `environmentAccounts` map, omitting the field entirely when every environment uses the owner account. Owners SHALL be able to edit description, owner email, secondary owner emails, and cost center from the domain detail page; all other attributes SHALL render read-only, including the effective environment → account map when the API provides it. Console links to per-environment domain resources SHALL resolve against the environment's effective account rather than the owner account. Platform admins SHALL be able to disable and re-enable domains from the admin domain list.

#### Scenario: Request submits as an async command

- **WHEN** an owner submits a valid request
- **THEN** the console SHALL POST the registration (including `owningTeam`) and show the accepted command with its request ID
- **AND THEN** the new domain SHALL appear in the caller's list as pending

#### Scenario: Override fields render per configured environment

- **WHEN** an owner opens the domain-request form on a platform configured with environments `dev`, `staging`, and `prod`
- **THEN** the form SHALL show one optional account field per environment
- **AND THEN** every field SHALL present empty as "use the owner account"

#### Scenario: Sparse overrides submit

- **WHEN** an owner fills only the `prod` override with a 12-digit account ID and submits
- **THEN** the request body SHALL carry `environmentAccounts: {"prod": "…"}` and no keys for untouched environments
- **AND THEN** a request with every override empty SHALL omit `environmentAccounts` entirely

#### Scenario: Malformed override is caught before submission

- **WHEN** an owner enters a value that is not a 12-digit account ID into an override field and submits
- **THEN** the console SHALL label the offending field with the format error and send no request

#### Scenario: Domain detail shows the effective account map

- **WHEN** the describe response carries `environmentAccounts`
- **THEN** the domain detail page SHALL render the effective environment → account map read-only
- **AND THEN** console links for an environment's domain-account resources SHALL target the environment's resolved account, so resources in an override account remain linked

#### Scenario: Owner edits only the mutable fields

- **WHEN** an owner saves changes from the domain detail edit form
- **THEN** the console SHALL submit only description, emails, and cost center
- **AND THEN** name, account, environments, and the account mapping SHALL have no editable affordance

#### Scenario: Cost center edit is pre-filled and validated

- **WHEN** an owner opens the domain detail edit form
- **THEN** the cost center field SHALL be pre-filled with the current value
- **AND THEN** an empty cost center SHALL be rejected client-side as the API contract requires

#### Scenario: Platform admin disables a domain

- **WHEN** a platform admin chooses disable on an ACTIVE domain and confirms
- **THEN** the console SHALL call the state endpoint and reflect INACTIVE
- **AND THEN** non-admin users SHALL never see the affordance

### Requirement: Approvals inbox

Platform admins SHALL review pending registrations from a dedicated inbox showing request details (name, description, owning team, owner account and emails, age). When the pending registration carries the resolved `environmentAccounts` map, the card SHALL show the effective account per environment so the approver sees where resources would land; registrations predating the field SHALL render without the map. Approve SHALL record the decision immediately; reject SHALL require a justification. Decided items SHALL leave the inbox on success, and failures (expired token, conflict) SHALL surface a descriptive error.

#### Scenario: Approve from the inbox

- **WHEN** a platform admin approves a pending registration
- **THEN** the console SHALL submit the decision with the registration's task token
- **AND THEN** the item SHALL be removed from the inbox

#### Scenario: Approval card shows the resolved account map

- **WHEN** a pending registration resolves `dev` and `staging` to the owner account and `prod` to another account
- **THEN** the approval card SHALL list all three environment → account pairs
- **AND THEN** a registration without the map SHALL render the remaining request details unchanged

#### Scenario: Rejection requires justification

- **WHEN** a platform admin attempts to reject without entering a justification
- **THEN** the console SHALL block submission and mark the justification field as required

### Requirement: Catalog discovery and subscription journeys

The console SHALL offer the entitlement-scoped catalog with producer and environment filters, schema detail (per-environment latest versions, JSON content), and a subscribe action that confirms the consumer domain and reports activation progress by polling until the subscription settles (surfacing the failure reason when one is recorded). When the schema is present in only some environments, the subscribe journey SHALL state which environments will materialize now and which will follow automatically on promotion. "My subscriptions" SHALL list inbound subscriptions with pause/resume/unsubscribe, SHALL render per-environment state including awaiting-promotion and per-environment failure with its recorded detail, and SHALL allow pausing or resuming environments that have not materialized yet; the domain detail page SHALL show outbound subscriptions (who consumes this domain's schemas). Producers SHALL see their own domain's schemas on the domain page without needing an entitlement.

#### Scenario: Subscribe confirms and tracks activation

- **WHEN** a team member subscribes their domain to a covered schema
- **THEN** the console SHALL submit the command, acknowledge acceptance (202), and poll the subscription
- **AND THEN** SHALL settle on the resulting per-environment states, on `FAILED` with the recorded reason, or on "still enabling" after the polling window

#### Scenario: Subscribe to a schema present in some environments

- **WHEN** a team member subscribes to a covered schema whose detail shows it exists in dev but not staging or prod
- **THEN** the console SHALL state that dev will activate now and the remaining environments will follow automatically when the schema is promoted
- **AND THEN** the journey SHALL still offer pause flags for environments that have not materialized

#### Scenario: Failure reason is visible

- **WHEN** a subscription or one of its environments is `FAILED`
- **THEN** the console SHALL display the recorded failure detail from the read API
- **AND THEN** no table or workflow access SHALL be needed to learn the reason

#### Scenario: Uncovered schema offers no subscribe action

- **WHEN** a schema is not covered for any of the caller's consumer domains
- **THEN** the schema detail SHALL explain that access requires an entitlement
- **AND THEN** no subscribe affordance SHALL be offered

#### Scenario: Pause and resume

- **WHEN** a team member pauses an `ENABLED` subscription environment
- **THEN** the console SHALL call the state endpoint and reflect the paused status
- **AND THEN** resuming SHALL re-enable delivery

#### Scenario: Pause an awaiting environment

- **WHEN** a team member pauses an environment that is awaiting promotion
- **THEN** the console SHALL call the state endpoint and reflect the paused desired state
- **AND THEN** when promotion later materializes the environment, it SHALL materialize disabled

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

The product SHALL have a design system documented at `docs/development/design-system.md` covering: design principles (clean, airy, professional; layered depth; generous whitespace; color-with-intent status semantics; `rounded-lg`/`rounded-xl` radius; `shadow-sm`-only elevation), the token set (color, type, spacing), core primitives (buttons, inputs, selects, badges/status pills, cards, tables, modal/confirm, toast, empty state), and page patterns (form, list+filter, detail, inbox). Tokens SHALL be implemented as Tailwind v4 `@theme` CSS variables with dark as the default theme, and every documented token SHALL correspond to a real token in code. A `/design-system` gallery route SHALL render the primitives for reference, including interactive overlays and applicable hover, focus, error, disabled, and loading states. The refined system SHALL retain the existing top navigation, page layouts, three surface levels, blue primary accent, and semantic status colors while applying consistent typography, spacing, borders, and control states.

#### Scenario: Dark theme is structural

- **WHEN** the console renders any screen
- **THEN** components SHALL consume semantic tokens and never raw palette utilities
- **AND THEN** the default theme SHALL be dark without any toggle or inversion pass

#### Scenario: Documentation matches implementation

- **WHEN** a token or primitive is documented in design-system.md
- **THEN** it SHALL exist in the codebase at the documented path
- **AND THEN** the gallery route SHALL render each documented primitive

#### Scenario: Existing journeys retain their structure

- **WHEN** a user visits a refreshed list, form, detail, or inbox page
- **THEN** its navigation, content grouping, and available product actions SHALL remain unchanged
- **AND THEN** shared controls SHALL use the refined visual system without changing status meanings

#### Scenario: Gallery demonstrates interaction states

- **WHEN** a reviewer opens the gallery
- **THEN** it SHALL provide normal, error, disabled, and loading examples where applicable and keyboard-operable modal, confirmation, and notification demonstrations
- **AND THEN** controls SHALL expose visible hover and keyboard focus states during interaction

### Requirement: Keyboard-accessible modal dialogs

Modal dialogs SHALL expose an accessible name, move focus inside on opening, contain keyboard focus while open, and prevent interaction with background content. Ordinary dialogs SHALL support dismissal through Escape, the backdrop, and a labeled close control. On closing, focus SHALL return to the opening control when it still exists and is focusable, or to a logical focusable location on the current page otherwise. Dialog content and actions SHALL remain reachable when content exceeds the viewport.

#### Scenario: Keyboard navigation stays in the dialog

- **WHEN** a user opens a modal and navigates with Tab or Shift+Tab
- **THEN** focus SHALL remain inside the modal and its title SHALL provide its accessible name
- **AND THEN** background controls SHALL not be operable until the modal closes

#### Scenario: Dismissal restores focus

- **WHEN** a user dismisses an ordinary modal with Escape, its backdrop, or its close control
- **THEN** the modal SHALL close without invoking its primary action
- **AND THEN** focus SHALL return to the opener or a logical fallback if the opener is unavailable

#### Scenario: Small viewport or long content

- **WHEN** modal content exceeds the available viewport at a narrow width or increased zoom
- **THEN** the user SHALL be able to scroll to all content and actions without interacting with the background page

### Requirement: Safe asynchronous confirmations

Confirmation dialogs SHALL name the action, expose its explanatory content as an accessible description, and initially focus Cancel rather than the confirming action. An idle confirmation SHALL allow cancellation with Cancel or Escape but SHALL NOT dismiss from a backdrop click. While its caller reports the action as busy, the dialog SHALL remain open, indicate progress, prevent duplicate confirmation, and prevent user-initiated cancellation or dismissal. The caller SHALL remain responsible for completing or failing the operation and controlling whether the dialog stays open.

#### Scenario: Cancellation does not perform the action

- **WHEN** a user opens a confirmation and chooses Cancel or presses Escape before confirming
- **THEN** the dialog SHALL report cancellation without invoking the confirming action
- **AND THEN** a backdrop click alone SHALL neither cancel nor confirm

#### Scenario: Busy confirmation cannot be dismissed or repeated

- **WHEN** a confirming action is in progress and the caller marks the confirmation busy
- **THEN** repeated activation, Escape, backdrop clicks, and cancellation controls SHALL not confirm again or dismiss the dialog
- **AND THEN** the confirming control SHALL display a busy state

#### Scenario: Failed operation can be retried or cancelled

- **WHEN** the caller clears busy after a failure and leaves the dialog open
- **THEN** confirmation and cancellation controls SHALL become available again
- **AND THEN** the explanatory content SHALL remain available

### Requirement: Accessible transient notifications

Transient notifications SHALL preserve success, informational, and error messaging, announce new messages politely without moving focus, and provide a labeled keyboard-accessible dismiss control. Notifications SHALL expire after six seconds of unpaused display time. Expiry SHALL pause while the notification region is hovered or contains keyboard focus, or while the browser window is inactive, and resume when those conditions no longer apply. Notifications SHALL remain within the viewport at narrow widths.

#### Scenario: Message appears without stealing focus

- **WHEN** an operation produces a transient notification
- **THEN** the message SHALL be announced politely and shown with its semantic tone
- **AND THEN** focus SHALL remain on the user's current control

#### Scenario: Reading or interacting pauses expiry

- **WHEN** a user hovers the notification region, focuses a control in it, or switches away from the browser window
- **THEN** notifications SHALL remain visible while the pause condition applies
- **AND THEN** automatic dismissal SHALL resume after interaction ends and the window is active

#### Scenario: Manual dismissal removes only the selected message

- **WHEN** a user activates a notification's dismiss control
- **THEN** only that notification SHALL be removed
- **AND THEN** other queued notifications SHALL remain independently dismissible

### Requirement: Accessible shared form control states

Shared inputs and selects SHALL associate labels and displayed hints or errors with their controls. They SHALL expose invalid and disabled states to assistive technology and retain visible keyboard focus indicators. Loading buttons SHALL prevent activation and communicate busy state without losing their action label. Simple selects SHALL retain native keyboard and form behavior, including existing empty-string filter values and required placeholders.

#### Scenario: Field error is associated with its control

- **WHEN** a shared input or select displays an error
- **THEN** the control SHALL expose an invalid state and reference the error as its accessible description
- **AND THEN** its label SHALL remain associated with the control

#### Scenario: Empty filter value remains selectable

- **WHEN** a user selects an existing empty-valued filter option such as "All environments"
- **THEN** the select SHALL retain the empty value and the page SHALL retain its existing unfiltered behavior

#### Scenario: Disabled and loading controls do not activate

- **WHEN** a user attempts to activate a disabled control or a loading button
- **THEN** no action SHALL be invoked
- **AND THEN** a loading button SHALL retain its label and expose its busy state

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

### Requirement: Platform metadata management interface

The console SHALL provide a platform metadata administration area. Administrators SHALL be able to inspect protected defaults, add generated fields and nested objects, preview a draft, review its differences from the latest revision, and save a new immutable revision. Auditors SHALL have read-only access. Domain-only users SHALL not receive configuration mutation controls, and backend authorization SHALL remain the enforcing boundary.

Each field SHALL display its type, generator, and validation semantics. Built-in and previously saved fields SHALL be locked against deletion and modification. The UI SHALL offer only supported generators, provide labeled field-level errors, preserve local edits after validation or conflict errors, and prevent duplicate submissions while busy. Existing shared controls and accessible dialog patterns SHALL be used.

#### Scenario: Administrator adds a generated field

- **WHEN** an administrator adds an `eventId` UUID field
- **THEN** the UI SHALL show its inferred type, generated-value example, validation constraints, and revision diff
- **AND THEN** saving SHALL create a revision without activating it

#### Scenario: Defaults remain protected

- **WHEN** an administrator views built-in metadata or fields inherited from a saved revision
- **THEN** the UI SHALL label them as mandatory or immutable
- **AND THEN** it SHALL not offer controls to remove or redefine them

#### Scenario: Preview has no live effects

- **WHEN** an administrator previews unsaved configuration
- **THEN** the UI SHALL show synthetic generated values and validation results for the selected environment context
- **AND THEN** preview SHALL NOT save configuration, publish an event, or move an active revision

#### Scenario: Save failure preserves work

- **WHEN** saving fails validation, authorization, or an outdated-predecessor check
- **THEN** the UI SHALL explain the failure and retain local edits
- **AND THEN** conflict recovery SHALL let the administrator reload the latest revision and review the difference

#### Scenario: Auditor inspects configuration

- **WHEN** a platform auditor opens metadata administration
- **THEN** defaults, revisions, history, and environment status SHALL be visible
- **AND THEN** save, activation, and other mutation controls SHALL be unavailable

### Requirement: Environment metadata activation interface

The console SHALL show the active metadata revision separately for each configured environment and allow administrators to explicitly select an environment for metadata activation. Revision history SHALL show definitions, authors, creation times, and activation audit records. Before activation, the UI SHALL show the target environment, current and candidate revisions, readiness results, affected current-schema inventory, and the warning that consumers must tolerate metadata additions. Readiness and inventory observations SHALL have timestamps and SHALL NOT be presented as proof of external consumer compatibility.

Activation SHALL use an explicit confirmation dialog naming the environment and revision. Busy state SHALL block dismissal and duplicate activation. Success SHALL refresh authoritative environment state; failure SHALL not optimistically show activation as complete. The environment selector is specific to metadata administration and SHALL NOT change the stage-derived target behavior of existing business-schema authoring.

#### Scenario: Activate only staging

- **WHEN** an administrator confirms activation of M2 for staging
- **THEN** the UI SHALL submit an activation for staging with the expected current revision
- **AND THEN** after success it SHALL show staging at M2 without changing dev or prod

#### Scenario: Activation races another administrator

- **WHEN** the server returns a stale-current-revision conflict
- **THEN** the UI SHALL reload authoritative activation state and require review before another submission
- **AND THEN** it SHALL not silently overwrite the other administrator's activation

### Requirement: Schema contract inspection in the console

Domain schema and catalog detail views SHALL show business versions and metadata revisions independently per environment. On the catalog detail view, each environment indicator SHALL be a selectable link that loads that environment's current contract, and the view SHALL present the raw business schema ("Schema content") and the complete composed document ("Schema preview") as full-width stacked panels, each with a copy-to-clipboard action for the rendered document, both resolved from the same selected environment. The Schema preview SHALL be captioned as the in-force contract that events in the selected environment validate against, identifying the business version, metadata revision, and definition digest. The console SHALL NOT expose historical pair selectors at this stage; historical inspection remains an API capability via the documented explicit `schemaVersion`/`metadataRevision` selectors. Producer schema editors SHALL keep metadata read-only and SHALL provide an effective-contract preview without requiring producers to author envelope fields, labelled as provisional and unregistered so it cannot be confused with the in-force Schema preview.

#### Scenario: Current schema reflects metadata activation

- **WHEN** staging moves from M1 to M2 while its business schema remains V3
- **THEN** the refreshed schema view SHALL display V3/M2 and the added metadata
- **AND THEN** it SHALL not claim that a business schema version was appended

#### Scenario: Environment selection drives both panels

- **WHEN** a reader selects a different environment indicator on the catalog detail view
- **THEN** the Schema content and Schema preview panels SHALL reload from that environment's current contract
- **AND THEN** the displayed business version and metadata revision SHALL be that environment's active pair

#### Scenario: In-force preview is not a provisional preview

- **WHEN** a reader views the catalog Schema preview panel
- **THEN** it SHALL be identified as the contract events in the selected environment currently validate against
- **AND THEN** the producer authoring preview SHALL remain labelled as unregistered and distinct from it

#### Scenario: Existing journeys retain behavior

- **WHEN** a publisher authors business content or a subscriber inspects a subscription
- **THEN** metadata inspection SHALL not alter business-schema append rules, entitlement checks, or subscription version pins
