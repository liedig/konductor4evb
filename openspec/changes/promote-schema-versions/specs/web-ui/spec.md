# Spec Delta

## ADDED Requirements

### Requirement: Producer schema page

The console SHALL provide a producer schema page at `/domains/{domain}/schemas/{schemaName}`. It SHALL show the environment pipeline in configured order, with each environment's latest version and how many versions it is behind its source. It SHALL also show the version history with the environments each version has reached, the subscribers in each environment with their pinned versions, and the latest promotion for each environment.

#### Scenario: Publisher opens the producer schema page

- **WHEN** a publisher on `orders` opens the page for `prezzee-orders@OrderNotification` with dev at v3, staging at v1, and prod at v1
- **THEN** the pipeline SHALL read dev v3, then staging v1 (2 behind dev), then prod v1 (in sync with staging)
- **AND THEN** the version history SHALL show v1 in dev, staging, and prod, and v2 and v3 in dev only

#### Scenario: Environment without the schema yet

- **WHEN** the schema exists in dev only
- **THEN** staging and prod SHALL appear in the pipeline as not yet promoted, rather than being omitted

#### Scenario: Subscribers by environment

- **WHEN** staging has `checkout` pinned at v1 and a paused `billing` pinned at v1
- **THEN** the page SHALL list both under staging with their pinned versions and paused state

#### Scenario: Caller without producer access

- **WHEN** a caller without a `publisher` binding on `orders` or a platform role opens the page
- **THEN** the page SHALL state that publisher access is required
- **AND THEN** no pipeline, history, or promotion data SHALL be shown

#### Scenario: Auditor sees the page read-only

- **WHEN** a `platform-auditor` opens the page
- **THEN** the page SHALL show the pipeline, history, subscribers, and promotions
- **AND THEN** no authoring or promote actions SHALL be rendered

### Requirement: Schema promotion journey

The producer schema page SHALL offer "Promote to {environment}" for each environment that is behind its source, to callers with a `publisher` binding. The confirmation SHALL name the source and target, the versions to replay, and how many target subscriptions will be re-pinned. After a `202` response, the page SHALL show the promotion's phase until it settles.

#### Scenario: Confirm and track a promotion

- **WHEN** a publisher confirms "Promote to staging" for a schema with dev at v3 and staging at v1
- **THEN** the confirmation SHALL state that v2 and v3 will be replayed into staging and how many staging subscriptions will be re-pinned to v3
- **AND THEN** after `202 Accepted` the staging node SHALL show replaying, then draining with the expected re-pin time, then in sync

#### Scenario: Confirmation follows the dialog contract

- **WHEN** the promotion confirmation is open
- **THEN** Cancel SHALL be the first action
- **AND THEN** dismissal SHALL be blocked while the request is in flight, and focus SHALL return to the promote button on close

#### Scenario: No promote action when in sync

- **WHEN** an environment's latest version equals its source's latest version
- **THEN** no promote action SHALL be offered for that environment

#### Scenario: Concurrent promotion is surfaced

- **WHEN** the API returns `409 Conflict` because a promotion into that environment is already running
- **THEN** the console SHALL report the conflict through a toast and show the running promotion's progress

#### Scenario: Divergence is explained

- **WHEN** a promotion fails with reason `DIVERGED`
- **THEN** the target node SHALL state that the environment's history no longer matches its source and that no versions were written

#### Scenario: Partial promotion is explained

- **WHEN** a promotion fails with reason `REPLAY_FAILED` after writing v2 of a requested v3
- **THEN** the page SHALL report that the environment and its subscribers are at v2, and SHALL offer promotion again

#### Scenario: Promotion progress survives navigation

- **WHEN** a publisher leaves and returns to the page while a promotion is draining
- **THEN** the page SHALL show the promotion's current phase from the status API

### Requirement: Environments render in configured order

Wherever the console lists a schema's environments (the producer schema page, the domain Schemas panel, and catalog schema detail), it SHALL order them by the runtime configuration's environment list, never alphabetically.

#### Scenario: Catalog detail follows pipeline order

- **WHEN** a consumer opens catalog detail for a schema present in dev, staging, and prod
- **THEN** the environment selector SHALL list dev, staging, prod in that order

#### Scenario: Custom environment names keep list order

- **WHEN** the configured list is `["sandbox", "qa", "live"]`
- **THEN** every environment display SHALL list sandbox, qa, live in that order

## MODIFIED Requirements

### Requirement: Schema authoring journeys

The producer's domain page schemas panel SHALL offer a "Register schema" action to callers with a `publisher` binding on that domain. The form SHALL accept an event name and JSON Schema draft-4 content; `schemaType` SHALL be fixed to `JSONSchemaDraft4` and SHALL NOT be selectable. Authoring SHALL always target the authoring environment, which is the first entry of the runtime configuration's environment list. The form SHALL display that environment and SHALL NOT offer an environment selector. Each listed schema SHALL offer an "append version" action with the same content editor. Each listed schema SHALL also show compact per-environment version badges in configured order and SHALL link to its producer schema page. Content SHALL be validated as parseable JSON client-side before submission. Success SHALL report the schema name and resulting version and SHALL refresh the panel's per-environment version badges. Failure responses SHALL be surfaced inline — `409 Conflict` for a duplicate schema, `403 Forbidden` for missing authorization, `400 Bad Request` with the server's validation detail — and SHALL NOT clear the form's entered content. Callers without a `publisher` binding SHALL see no authoring affordances.

#### Scenario: Publisher registers a schema

- **WHEN** a publisher submits the register form with an event name and valid draft-4 content
- **THEN** the console SHALL create the schema in the authoring environment and display its canonical name and version 1
- **AND THEN** the panel SHALL refresh to show the new schema with its environment badges

#### Scenario: Publisher appends a version

- **WHEN** a publisher submits new content via a schema's append-version action
- **THEN** the console SHALL append the version in the authoring environment and display the new version number
- **AND THEN** the panel's version badge for the authoring environment SHALL update

#### Scenario: Malformed JSON caught before submission

- **WHEN** the content editor holds text that is not parseable JSON
- **THEN** the console SHALL block submission and identify the parse error
- **AND THEN** no API call SHALL be made

#### Scenario: Server validation failure is surfaced and content is retained

- **WHEN** the API rejects a submission with `400 Bad Request` (for example, content that is not a valid draft-4 document)
- **THEN** the console SHALL display the server's validation detail inline
- **AND THEN** the form SHALL retain the entered event name and content

#### Scenario: Duplicate schema surfaces conflict

- **WHEN** the API returns `409 Conflict` because the derived schema name already exists in the authoring environment's registry
- **THEN** the console SHALL display the conflict inline
- **AND THEN** the form SHALL retain the entered content

#### Scenario: Target environment is shown, not chosen

- **WHEN** a publisher opens either authoring form
- **THEN** the form SHALL display the authoring environment as the target
- **AND THEN** no environment input SHALL be offered

#### Scenario: Panel links to the producer schema page

- **WHEN** a publisher views the domain Schemas panel
- **THEN** each schema SHALL show badges such as `dev v3`, `staging v1`, `prod v1` in configured order
- **AND THEN** selecting the schema SHALL open its producer schema page

#### Scenario: Non-publisher sees no authoring affordances

- **WHEN** a caller without a `publisher` binding on the domain views the domain page
- **THEN** the register and append-version actions SHALL NOT be rendered
- **AND THEN** the read-only schema list behavior SHALL remain unchanged

### Requirement: Static hosting and deployment

The console SHALL build to static assets deployed to an S3 bucket behind a CloudFront distribution with TLS, SPA fallback (403/404 → `index.html`), and response security headers including a restrictive content-security-policy. CloudFront SHALL route API requests for every configured environment to that environment's OAuth API stage, so the console can reach each environment's stage from the same origin without a per-environment distribution change. Deployment SHALL write `config.json` from current stack outputs (OAuth API URL, Cognito pool/client IDs, and the environment list). The authoring stage SHALL be the first environment in that list, not a fixed name. Deployment SHALL register the web origins in the stack's OAuth callback/logout URLs.

#### Scenario: Deploy publishes config from the stack

- **WHEN** an operator runs the web deploy target
- **THEN** the distribution SHALL serve the latest build
- **AND THEN** `config.json` SHALL reflect the current stack outputs without a rebuild

#### Scenario: Every environment stage is routed

- **WHEN** the installation's environment list is `["sandbox", "qa", "live"]`
- **THEN** console requests addressed to `sandbox`, `qa`, or `live` SHALL reach the OAuth API stage of the same name
- **AND THEN** authoring requests SHALL target the `sandbox` stage

#### Scenario: Deep links survive refresh

- **WHEN** a signed-in user refreshes on a deep route such as `/domains/orders`
- **THEN** CloudFront SHALL serve the SPA
- **AND THEN** the router SHALL restore the route after session revalidation
