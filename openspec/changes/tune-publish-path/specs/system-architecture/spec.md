# Spec Delta

## MODIFIED Requirements

### Requirement: Platform configuration contract

The platform SHALL define a configuration contract covering every customer-supplied setting: organisation identifier, AWS Organization ARN, domain environment list, resource namespace pattern, notification sender and operator recipient addresses, access-token validity, OAuth callback/logout URLs, and the publish-path capacity settings (publish throttle rate and burst, admission concurrency cap, delivery concurrency limit, and domain liveness window). Each setting SHALL be classified as fixed-at-setup or updateable. The organisation identifier, AWS Organization ARN, domain environment list, and namespace pattern SHALL be fixed at setup. Notification addresses, access-token validity, OAuth URLs, and the publish-path capacity settings SHALL be updateable by redeployment with new values. The platform SHALL remain a single organisation-level control plane; the environment list SHALL describe domain environments only, not separate platform instances.

Deployment tooling SHALL refuse to apply a changed fixed-at-setup value to an existing installation and SHALL report which setting changed and that a new deployment is required. Tooling SHALL NOT rename, replace, orphan, or delete existing resources as a side effect of such a change.

#### Scenario: Fixed-at-setup change is rejected

- **WHEN** an operator deploys with an organisation identifier, environment list, namespace pattern, or AWS Organization ARN that differs from the deployed stack
- **THEN** deployment tooling SHALL refuse before any resource change
- **AND THEN** the refusal SHALL name the differing setting and state that changing it requires a new deployment

#### Scenario: Updateable setting changes by redeployment

- **WHEN** an operator redeploys with new notification addresses, token validity, OAuth URLs, or publish-path capacity settings
- **THEN** the new values SHALL be validated
- **AND THEN** the update SHALL apply without a fixed-at-setup rejection

#### Scenario: Single organisation-level control plane

- **WHEN** the environment list is configured
- **THEN** it SHALL describe the environments approved domains are provisioned into
- **AND THEN** it SHALL NOT create additional platform instances or environment-qualified platform resource names

### Requirement: Pre-deployment validation with actionable feedback

The platform SHALL provide a supported pre-deployment validation step that checks every contract setting and reports actionable errors before any deployment occurs. Validation SHALL cover: environment entry syntax, uniqueness, and count; namespace pattern required placeholders; organisation identifier syntax; AWS Organization ARN format; notification address pairing and SES verification prerequisites; HTTPS for OAuth URLs outside local development; and publish-path capacity setting ranges. Deployment through the supported path SHALL NOT proceed when validation fails.

#### Scenario: Invalid environment entry fails before deploy

- **WHEN** the configuration contains an environment entry that violates the documented syntax
- **THEN** validation SHALL fail before any deployment
- **AND THEN** the error SHALL name the offending entry and the syntax rule

#### Scenario: Notifications are explicitly enabled or disabled

- **WHEN** exactly one notification address is configured, or a configured address is not verified in SES
- **THEN** validation SHALL fail with an actionable message
- **WHEN** both notification addresses are empty
- **THEN** validation SHALL pass and report that notifications are disabled

#### Scenario: Out-of-range publish-path setting fails before deploy

- **WHEN** a publish-path capacity setting is outside its documented range, for example a delivery concurrency limit below 2
- **THEN** validation SHALL fail before any deployment
- **AND THEN** the error SHALL name the setting and its allowed range

## ADDED Requirements

### Requirement: Publish throttle budget warning

Pre-deployment validation SHALL compare the configured API Gateway limits with the account's API Gateway throttle quota in the deployment Region. It SHALL warn, without failing, when one stage's publish limit plus every stage's control-plane limit exceeds the account rate or burst. It SHALL also report the total of every stage's limits against the quota as a note. The account burst SHALL be taken as half the rate quota, because AWS derives it from the rate.

#### Scenario: Default configuration fits the default account quota

- **WHEN** validation runs with the default publish throttle, the default three environments, and an account throttle quota of 10,000 requests per second
- **THEN** validation SHALL NOT warn about the throttle budget
- **AND THEN** it SHALL report that all stages at their limits total 12,300 requests per second with a burst of 6,600, against an account quota of 10,000 with a burst of 5,000

#### Scenario: One stage can exceed a lower account quota

- **WHEN** validation runs with the default publish throttle and three environments, and the account throttle quota is 2,500 requests per second
- **THEN** validation SHALL warn that one stage's publish burst plus every stage's control-plane burst (1,600) exceeds the account burst of 1,250
- **AND THEN** the warning SHALL name the publish throttle settings, and validation SHALL NOT fail

#### Scenario: Quota cannot be read

- **WHEN** the account throttle quota cannot be read
- **THEN** validation SHALL compare against the documented default of 10,000 requests per second with a burst of 5,000
- **AND THEN** it SHALL report that it used the default

#### Scenario: Warnings do not block deployment

- **WHEN** validation finds only throttle budget warnings
- **THEN** validation SHALL succeed, and deployment through the supported path SHALL proceed
