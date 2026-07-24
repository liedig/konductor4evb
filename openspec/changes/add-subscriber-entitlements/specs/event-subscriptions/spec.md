## ADDED Requirements

### Requirement: Consumer-domain subscription ownership

A subscription SHALL be owned by its consumer domain, not by the user, Cognito group, or principal that created it. Any caller with current subscriber access to the consumer domain, or a global `platform-admin`, SHALL operate its subscriptions subject to entitlement requirements. Nexus SHALL manage only the bus-to-bus EventBridge rules that deliver from the producer's platform-side domain bus to the consumer's app-side domain bus; rules downstream of that app-side bus SHALL remain outside Nexus ownership.

#### Scenario: Another subscriber operates a domain-owned subscription

- **WHEN** a caller has current subscriber access to a consumer domain but did not create one of its subscriptions
- **THEN** the caller SHALL be permitted to perform an otherwise authorized operation on that subscription

#### Scenario: Downstream rules remain application-owned

- **WHEN** Nexus creates, disables, enables, or removes a subscription
- **THEN** Nexus SHALL NOT create, update, or delete rules downstream of the consumer's app-side domain bus

### Requirement: Subscription API terminology

The public API SHALL use the term `subscriptions` for subscription resources and response fields and SHALL NOT expose them as `event-streams`.

#### Scenario: Public subscription response

- **WHEN** a caller lists or describes subscription resources
- **THEN** the API SHALL identify those resources as `subscriptions`
- **AND THEN** the response SHALL NOT use `event-streams` as their public resource name

## MODIFIED Requirements

### Requirement: Schema-first subscription

Consumers SHALL subscribe a registered consumer domain to another registered domain's event schema by sending `POST /domains/{consumerDomain}/subscriptions` with `producerDomain` and `schemaName`. The consumer domain SHALL come only from the path and SHALL NOT be accepted in the request body. The subscription SHALL bind to the **latest** version of the schema in each environment's registry, resolved and pinned at subscribe time. The API SHALL NOT allow subscribing to a historical schema version and SHALL NOT accept a caller-supplied event pattern. Creation SHALL require the caller to have subscriber access to the consumer domain and the consumer domain to have a current entitlement covering the producer schema.

#### Scenario: Subscribe to an entitled schema

- **WHEN** an authorized subscriber sends `POST /domains/checkout/subscriptions` with `producerDomain: orders` and `schemaName: prezzee-orders@OrderNotification`
- **THEN** the control plane SHALL resolve the latest version of `prezzee-orders@OrderNotification` in each of the producer's per-environment schema registries
- **AND THEN** pin the resolved version per environment in the subscription's EventBridge rules and subscription record

#### Scenario: Consumer domain cannot be supplied in the body

- **WHEN** a caller includes `consumerDomain` in a create or update request body
- **THEN** the request SHALL be rejected by structural validation

#### Scenario: Historical version cannot be subscribed

- **WHEN** a caller attempts to specify a schema version in a subscribe request
- **THEN** the request SHALL be rejected by structural validation because the subscribe operation accepts no version parameter

#### Scenario: Per-environment version skew is recorded faithfully

- **WHEN** the latest version of the schema differs across environments
- **THEN** each environment's rule SHALL pin that environment's latest version
- **AND THEN** the subscription record SHALL show the pinned version per environment

### Requirement: Subscription identity and idempotency

A subscription SHALL be identified by the triple (consumerDomain, producerDomain, detailType), exposed as `{consumerDomain}.{producerDomain}.{detailType}`. Domain names cannot contain dots, so the identifier SHALL be parsed by splitting on the first two dots; the event name portion may itself contain dots. There SHALL be at most one subscription per triple. A create request for an existing active subscription SHALL be idempotent. On a given producer bus, the rule name `{consumerDomain}-{detailType}` is bijective with the subscription identity, and rule creation SHALL be an upsert (`PutRule`) so retried workflow steps are safe.

#### Scenario: Duplicate create is idempotent

- **WHEN** a caller creates a subscription and an `ACTIVE` subscription already exists for the same consumer, producer, and detail type
- **THEN** the operation SHALL succeed without creating duplicate resources
- **AND THEN** the outcome SHALL reference the existing subscription

#### Scenario: Re-create after removal creates a fresh contract

- **WHEN** a caller creates a subscription and a `REMOVED` record exists for the same triple
- **THEN** a new subscription SHALL be created, re-resolving the latest schema versions at that time

### Requirement: Per-environment enable and disable

A subscription SHALL carry per-environment desired state (`enabled: true|false`) mapped to the native EventBridge rule state (`ENABLED|DISABLED`). Initial per-environment state SHALL be settable in the create request, defaulting to enabled in every environment. State SHALL be updatable through `PATCH /domains/{consumerDomain}/subscriptions/{subscriptionId}` without accepting `consumerDomain` in the body. A caller with subscriber access to the consumer domain, or a global `platform-admin`, SHALL always be allowed to disable the subscription. Enabling SHALL additionally require the consumer domain to have a current entitlement covering the subscription's producer schema. Disabling SHALL NOT remove the rule, unpin the version, or void the contract.

#### Scenario: Create with production paused

- **WHEN** an authorized subscriber creates a subscription with `environments: {prod: {enabled: false}}`
- **THEN** the prod rule SHALL be created with state `DISABLED` while the other environment rules are `ENABLED`

#### Scenario: Disable after entitlement revocation

- **WHEN** an authorized subscriber patches a subscription to disable one or more environments after its last covering entitlement was revoked
- **THEN** Nexus SHALL accept the disable operation
- **AND THEN** stop delivery in the selected environments without removing rules or pinned versions

#### Scenario: Enable requires a current entitlement

- **WHEN** an authorized subscriber patches a disabled subscription to enable an environment
- **THEN** Nexus SHALL enable it only if the consumer domain has a current entitlement covering the producer schema

### Requirement: Unsubscribe

Deleting a subscription through `DELETE /domains/{consumerDomain}/subscriptions/{subscriptionId}` SHALL remove its rules, targets first and then rules, from the producer's platform bus in every environment. Removal SHALL be idempotent, treating already-deleted resources as no-ops. A caller with subscriber access to the consumer domain, or a global `platform-admin`, SHALL be allowed to delete the subscription even after its entitlement has been revoked. The subscription record SHALL be marked `REMOVED` with a `removedAt` timestamp and retained for 90 days through DynamoDB TTL. The control plane SHALL NOT touch the consumer's app-side bus.

#### Scenario: Clean deletion

- **WHEN** an authorized subscriber deletes a subscription under its consumer domain
- **THEN** all per-environment rules for the subscription SHALL be removed from the producer's platform buses
- **AND THEN** the subscription record SHALL have status `REMOVED` and an expiry 90 days after removal

#### Scenario: Delete remains available after revocation

- **WHEN** an authorized subscriber deletes a subscription after the last entitlement covering it has been revoked
- **THEN** Nexus SHALL accept and process the deletion

#### Scenario: Orphaned consumer rules are the application team's responsibility

- **WHEN** a subscription is removed and the consumer's app-side bus still carries integration rules for the removed delivery
- **THEN** the control plane SHALL NOT modify or delete those rules

### Requirement: Subscription validation and authorization

Before activating a subscription, the control plane SHALL validate that both consumer and producer domains exist and are `ACTIVE`, the caller has subscriber access to the consumer domain or the OAuth global `platform-admin` role, the consumer domain has a current producer-domain or exact-schema entitlement covering the producer schema, and the derived rule name satisfies EventBridge constraints. Self-subscription SHALL be permitted when these authorization conditions hold. Subscriber access through a Cognito team-group or IAM AWS-account binding SHALL govern inbound reads and mutations within a consumer domain. A caller with publisher access through either principal type to a producer domain SHALL have full read-only visibility of that domain's outbound subscriptions. OAuth global `platform-admin` and `platform-auditor` roles SHALL inspect all subscriptions; a platform administrator SHALL mutate subscriptions across domains subject to current entitlement rules, while a `platform-auditor` SHALL NOT mutate them. Unauthorized subscription detail requests SHALL return `404 Not Found`.

#### Scenario: Subscriber and entitlement are required to create

- **WHEN** a caller lacks subscriber access to the consumer domain or the domain lacks a current covering entitlement
- **THEN** the create command SHALL fail authorization

#### Scenario: Publisher can inspect but not mutate outbound subscriptions

- **WHEN** a caller has publisher access to a producer domain
- **THEN** the caller SHALL be able to read every outbound subscription for that producer domain
- **AND THEN** the caller SHALL NOT gain mutation rights to those subscriptions

#### Scenario: Inactive domain rejected

- **WHEN** either domain in a create request is not in `ACTIVE` status
- **THEN** the command SHALL fail validation identifying the offending domain and its current status

#### Scenario: Rule name budget exceeded

- **WHEN** `{consumerDomain}-{detailType}` exceeds 64 characters or contains characters outside `[A-Za-z0-9._-]`
- **THEN** the create command SHALL fail validation with an error explaining the constraint

#### Scenario: Self-subscription permitted

- **WHEN** an authorized consumer domain subscribes to one of its own entitled schemas
- **THEN** the subscription SHALL be created normally

#### Scenario: Unauthorized detail is concealed

- **WHEN** a caller requests `GET /subscriptions/{subscriptionId}` without subscriber, publisher, or global platform visibility for that subscription
- **THEN** the API SHALL return `404 Not Found`

### Requirement: Subscription read operations

The control plane SHALL provide `GET /domains/{domain}/subscriptions?direction=inbound|outbound` and canonical `GET /subscriptions/{subscriptionId}` operations on both OAuth and IAM API surfaces, served synchronously from the subscriptions registry through shared handlers and decisions. `direction` SHALL be required. The inbound view SHALL return subscriptions owned by the consumer domain and require subscriber access to that domain through a Cognito team-group or IAM AWS-account binding. The outbound view SHALL return full subscription visibility for the producer domain and require publisher access through either binding principal type. OAuth global `platform-admin` and `platform-auditor` callers SHALL inspect either view and canonical detail across all domains. List operations SHALL support filters for schema name, pinned version, and status. Detail SHALL include consumer, producer, schema, per-environment pinned versions, rule ARNs, enabled flags, and materialization status.

#### Scenario: Subscriber lists inbound subscriptions

- **WHEN** a caller with subscriber access to `checkout` requests `GET /domains/checkout/subscriptions?direction=inbound`
- **THEN** the response SHALL list subscriptions whose consumer domain is `checkout`

#### Scenario: Publisher lists complete outbound subscriptions

- **WHEN** a caller with publisher access to `orders` requests `GET /domains/orders/subscriptions?direction=outbound`
- **THEN** the response SHALL list every subscription whose producer domain is `orders`, including consumer domain and per-environment state

#### Scenario: IAM account principal lists subscriptions

- **WHEN** an IAM caller's trusted AWS account has the required subscriber or publisher binding for the requested domain
- **THEN** the corresponding inbound or outbound request on the IAM surface SHALL return the same domain-scoped view as an equivalently authorized OAuth caller

#### Scenario: Direction is required

- **WHEN** a caller requests the domain-scoped subscription collection without `direction`
- **THEN** the API SHALL reject the request with `400 Bad Request`

#### Scenario: Describe a visible subscription

- **WHEN** an authorized caller requests `GET /subscriptions/checkout.orders.OrderNotification`
- **THEN** the response SHALL include the consumer, producer, schema, full per-environment map, overall status, and timestamps

