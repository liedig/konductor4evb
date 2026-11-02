# Spec Delta

## MODIFIED Requirements

### Requirement: Platform-managed domain enable and disable

The control plane SHALL let the platform team disable and re-enable an `ACTIVE`/`INACTIVE` domain via `POST /domains/{domainName}/state` restricted to the `platform-admin` global role. While a domain is `INACTIVE`, the control plane SHALL reject new subscription activations and entitlement and subscription administration for the domain, SHALL reject publishes once the domain liveness window elapses, and SHALL retain existing subscriptions, entitlements, and provisioned infrastructure unchanged.

#### Scenario: Platform admin disables a domain

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/{domainName}/state` with action `disable` on an `ACTIVE` domain
- **THEN** the control plane SHALL transition the domain to `INACTIVE`
- **AND THEN** subsequent subscribe-activation attempts for the domain SHALL be rejected with a descriptive error
- **AND THEN** publish attempts for the domain made after the domain liveness window has elapsed since the disable SHALL be rejected with a descriptive error, and no queued event for the domain SHALL be delivered after that point

#### Scenario: Platform admin re-enables a domain

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/{domainName}/state` with action `enable` on an `INACTIVE` domain
- **THEN** the control plane SHALL transition the domain to `ACTIVE`
- **AND THEN** publishing and subscription activation SHALL resume without waiting for the domain liveness window

#### Scenario: Non-admin callers cannot change domain state

- **WHEN** a caller without the `platform-admin` role calls `POST /domains/{domainName}/state`
- **THEN** the control plane SHALL return `403` access denied
- **AND THEN** the domain status SHALL remain unchanged

#### Scenario: Disable retains existing estate

- **WHEN** a domain with active subscriptions and provisioned infrastructure is disabled
- **THEN** those subscriptions and resources SHALL remain in place and unchanged
- **AND THEN** only new publish and activation activity SHALL be rejected
