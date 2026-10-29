# Spec Delta

## ADDED Requirements

### Requirement: SDK generation detects contract drift

SDK validation SHALL derive the supported operation subset from the maintained OAuth API contract, stripping authentication and deployment integration. Regeneration with pinned inputs SHALL reproduce committed generated artifacts. Changes to unrelated operations, or to the IAM contract, SHALL NOT change the SDK.

#### Scenario: Covered route changes in the OAuth contract
- **WHEN** a selected operation's request or response shape changes in the OAuth contract
- **THEN** the regeneration check SHALL fail until the projection and generated client are regenerated
- **AND** authentication and deployment-integration details SHALL NOT appear in the projection

#### Scenario: Generated artifacts are stale
- **WHEN** a covered contract or generator input changes without the matching generated artifacts
- **THEN** the regeneration check SHALL fail rather than silently package stale code

#### Scenario: Platform-only route is added
- **WHEN** a maintained contract gains a platform-administration operation
- **THEN** the SDK subset SHALL remain restricted to its four supported operations and their dependencies

### Requirement: Credential-free SDK quality gates

CI SHALL run SDK contract checks, reproducibility checks, Python quality checks, behavior tests, and distribution build/install checks through documented local-equivalent commands. Tests SHALL use isolated transports and synthetic credentials. CI SHALL NOT call live Konductor APIs, provision AWS resources, or publish SDK packages.

#### Scenario: SDK behavior regresses
- **WHEN** an SDK change breaks a request fixture, response fixture, authentication assertion, JSON round trip, or package import
- **THEN** the SDK CI gate SHALL fail

#### Scenario: Contributor has no AWS account access
- **WHEN** SDK checks run in a clean environment without live AWS credentials or Cognito tokens
- **THEN** all default SDK tests SHALL remain runnable without resolving ambient credentials or contacting identity services

#### Scenario: Built package is independently installable
- **WHEN** CI builds the source distribution and wheel
- **THEN** it SHALL verify that the source distribution can build a wheel and that an installed wheel exposes the supported interface outside the checkout
- **AND** package installation SHALL NOT require the generator toolchain

### Requirement: Reusable SDK wire conformance evidence

The repository SHALL maintain language-neutral request and response fixtures for the supported SDK operations. Python conformance tests SHALL exercise those fixtures through the public client interface. Fixtures SHALL describe API behavior independently of the selected generator so future runtime SDKs can use the same evidence.

#### Scenario: Replace the generator or add a runtime
- **WHEN** an SDK implementation changes its generator or a new language consumes the fixtures
- **THEN** the fixture meanings SHALL remain independent of generated class names or generator-specific serialization
- **AND** expectations SHALL continue to distinguish acceptance, materialization state, and failures

#### Scenario: Fixture payloads cover real response behavior
- **WHEN** the SDK conformance suite runs
- **THEN** it SHALL cover arbitrary JSON data, structured and unstructured errors, incomplete acceptance responses, nullable fields, and partial subscription activation
- **AND** it SHALL use synthetic data without secrets or user-generated event content
