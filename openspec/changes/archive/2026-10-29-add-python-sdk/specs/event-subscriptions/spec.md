# Spec Delta

## ADDED Requirements

### Requirement: Python SDK subscription commands

The Python SDK SHALL expose `subscribe` with consumer domain, producer domain, schema name, and optional per-environment desired state, and `unsubscribe` with subscription identifier. Both SHALL use the existing commands and return acceptance receipts, not completed subscription resources. Neither operation SHALL receive events or provision application-side integrations.

#### Scenario: Subscribe with one environment paused
- **WHEN** a caller subscribes with production disabled and other environments omitted
- **THEN** the SDK SHALL preserve the explicit disabled value and omit unprovided environments
- **AND** it SHALL leave environment defaults, version selection, authorization, and activation to the service

#### Scenario: Accepted command can later fail
- **WHEN** subscribe or unsubscribe returns HTTP 202 with a nonempty `requestId`
- **THEN** the SDK SHALL return the request identifier as an acceptance receipt
- **AND** it SHALL NOT assert that resource authorization passed or activation or removal completed

#### Scenario: Unsubscribe preserves dotted event names
- **WHEN** a caller unsubscribes using `checkout.orders.order.placed.v2`
- **THEN** the SDK SHALL address the existing subscription-detail path with that complete identifier
- **AND** it SHALL NOT truncate the event name, send a replacement request body, or infer a different subscription

### Requirement: Python SDK subscription detail inspection

The Python SDK SHALL expose `describe_subscription` as a synchronous read of the existing detail route. It SHALL preserve the returned identity, timestamps, overall status, failure detail, and per-environment pinned versions, rule information, desired state, and materialization status. Absent or nullable fields SHALL NOT cause valid responses to fail decoding.

#### Scenario: Partial activation is visible
- **WHEN** detail reports overall `ACTIVE`, a materialized dev environment, and prod `AWAITING_PROMOTION` without a pin or rule ARN
- **THEN** the SDK SHALL preserve those distinct states and missing values
- **AND** it SHALL NOT describe all environments as active

#### Scenario: Failures and removal are visible
- **WHEN** detail contains top-level or per-environment `errorDetail`, or a `removedAt` value
- **THEN** the SDK SHALL expose the supplied information
- **AND WHEN** the top-level nullable fields are null or optional environment fields are omitted
- **THEN** the SDK SHALL decode the response without inventing values

#### Scenario: Additive response evolution
- **WHEN** the service adds response fields or returns an unfamiliar status string
- **THEN** the SDK SHALL tolerate additional fields and preserve the status value without treating it as successful activation

#### Scenario: Detail is unavailable or forbidden
- **WHEN** detail returns HTTP 404 or 403
- **THEN** the SDK SHALL raise the corresponding service error
- **AND** it SHALL NOT convert the response into a removed subscription or successful unsubscribe

### Requirement: SDK inspection is not command completion tracking

The Python SDK SHALL document subscription detail as current resource state rather than a result correlated to an acceptance receipt. Its first interface SHALL NOT include automatic activation/removal waiters or command-status polling. Examples SHALL separate command submission, inspection, and application-side event consumption.

#### Scenario: Existing active resource follows a new command
- **WHEN** a caller submits a command and then reads a previously active subscription
- **THEN** the SDK SHALL return the observed state without claiming that the new command succeeded

#### Scenario: Demo uses existing infrastructure
- **WHEN** an application-team example demonstrates subscribing, inspecting, publishing, and unsubscribing
- **THEN** it SHALL state the preconfigured domain, schema, binding, entitlement, and event-consumption prerequisites
- **AND** it SHALL NOT silently provision resources, acquire elevated roles, or label command acceptance as readiness
