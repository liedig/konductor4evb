# Delta: web-ui — follow-schema-promotion

## MODIFIED Requirements

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
