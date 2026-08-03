## MODIFIED Requirements

### Requirement: Subscription validation and authorization

Before activating a subscription, the control plane SHALL validate that: both the consumer and producer domains exist and are `ACTIVE`; the derived rule name satisfies EventBridge constraints (charset `[A-Za-z0-9._-]`, length ≤ 64 characters); and the grant engine allows the subscribe action for the normalized trusted principal against the composite resource `{consumerDomain}/{producerDomain}/{eventName}`. Account-ownership matching against the consumer domain's `ownerAccountId` SHALL be removed. Create and enable SHALL require a `subscriber` binding on the consumer domain plus a current covering entitlement; disable and delete SHALL require only the binding. Self-subscription (consumer equals producer) SHALL be permitted and implicitly covered — it is how a domain consumes its own events on its app-side bus. `platform-admin` MAY operate subscriptions without a subscriber binding while still requiring current covering entitlement for create and enable. Unauthorized attempts SHALL fail with `403 Forbidden` and a deterministic `*Failed` outcome event; deny-as-unknown concealment SHALL NOT be applied.

#### Scenario: Covered subscriber creates a subscription

- **WHEN** a caller holds a `subscriber` binding on the consumer domain and the consumer domain holds a current covering entitlement for the target producer schema
- **THEN** the subscribe command SHALL pass authorization and proceed to validation and activation

#### Scenario: Missing binding or entitlement denied

- **WHEN** a caller lacks a `subscriber` binding on the consumer domain, or no current entitlement covers the target producer schema
- **THEN** the command SHALL fail with `403 Forbidden`
- **AND THEN** a `SubscribeFailed` outcome event SHALL carry the authorization failure without tokens, claims, or payload detail

#### Scenario: Account ownership no longer authorizes

- **WHEN** a caller whose AWS account matches the consumer domain's `ownerAccountId` but who holds no `subscriber` binding attempts to subscribe
- **THEN** the command SHALL fail authorization with `403 Forbidden`

#### Scenario: Post-revocation lifecycle control retained

- **WHEN** the entitlement covering an existing subscription has been revoked
- **THEN** an authorized subscriber SHALL remain able to disable or delete the subscription
- **AND THEN** create or enable SHALL be denied

#### Scenario: Inactive domain rejected

- **WHEN** either domain in a subscribe request is not in `ACTIVE` status
- **THEN** the command SHALL fail validation identifying the offending domain and its current status

#### Scenario: Rule name budget exceeded

- **WHEN** `{consumerDomain}-{detailType}` exceeds 64 characters or contains characters outside `[A-Za-z0-9._-]`
- **THEN** the subscribe command SHALL fail validation with an error explaining the constraint

#### Scenario: Self-subscription permitted

- **WHEN** a caller with a `subscriber` binding on a domain subscribes that domain to one of its own schemas
- **THEN** the subscription SHALL be created normally without any entitlement, delivering the domain's events to its own app-side bus

### Requirement: Subscription read operations

The control plane SHALL provide `GET /subscriptions?domain={domain}&direction=inbound|outbound` and canonical `GET /subscriptions/{subscriptionId}`. The `direction` parameter SHALL be required; the global list route SHALL NOT exist. Inbound SHALL list subscriptions where the queried domain consumes, authorized by a `subscriber` binding on that domain. Outbound SHALL list subscriptions consuming the queried producer domain's schemas, authorized by a `publisher` binding on that domain, and SHALL return full consumer topology because producer teams need it for notifications and future version migration. Both SHALL support schema, pinned-version, and status filters and SHALL be served synchronously from the event-streams store. Canonical detail SHALL authorize against the visible consumer side, the visible producer side, or a platform role (`platform-admin`, `platform-auditor`), returning full stream detail including per-environment pinned versions, rule ARNs, enabled flags, and materialization status. Unauthorized detail or list requests SHALL return `403 Forbidden`; a missing subscription SHALL return `404 Not Found`.

#### Scenario: Subscriber lists inbound subscriptions

- **WHEN** a caller with a `subscriber` binding on `checkout` requests `GET /subscriptions?domain=checkout&direction=inbound`
- **THEN** the response SHALL list `checkout`'s subscriptions with the requested filters applied

#### Scenario: Publisher lists outbound consumers

- **WHEN** a caller with a `publisher` binding on `orders` requests `GET /subscriptions?domain=orders&direction=outbound`
- **THEN** the response SHALL list all subscriptions consuming `orders` schemas, including consumer domains and pinned versions

#### Scenario: Direction is required

- **WHEN** a caller requests `GET /subscriptions?domain=checkout` without `direction`
- **THEN** the API SHALL reject the request identifying the required parameter

#### Scenario: Describe a subscription from an authorized side

- **WHEN** a caller authorized through the consumer side, the producer side, or a platform role requests `GET /subscriptions/checkout.orders.OrderNotification`
- **THEN** the response SHALL include the full per-environment map (pinned version, rule ARN, enabled, status), overall status, and timestamps

#### Scenario: Unauthorized detail returns 403

- **WHEN** a caller with no authorization on either side requests an existing subscription's detail
- **THEN** the API SHALL return `403 Forbidden`
- **AND THEN** a request for a subscription that does not exist SHALL return `404 Not Found`

## ADDED Requirements

### Requirement: Final entitlement check before enablement

Immediately before any workflow transitions a Nexus-managed EventBridge rule to `ENABLED`, including initial activation and per-environment enable operations, the control plane SHALL perform a final current-entitlement check through the grant engine for the subscription's consumer domain and producer schema. If no current entitlement covers the schema at that moment, the workflow SHALL NOT enable the rule and SHALL record the denial.

#### Scenario: Revocation during activation blocks enablement

- **WHEN** the covering entitlement is revoked after a subscribe command was accepted but before its activation workflow enables a rule
- **THEN** the workflow SHALL NOT transition any rule to `ENABLED`
- **AND THEN** the workflow SHALL record the entitlement denial as the outcome

#### Scenario: Enablement proceeds while covered

- **WHEN** a current covering entitlement exists at the moment of the final check
- **THEN** the workflow SHALL enable the rule as designed
