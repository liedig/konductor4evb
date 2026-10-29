# Spec Delta

## ADDED Requirements

### Requirement: Python SDK publication preserves business data

The Python SDK SHALL expose `publish` with required domain, event name, and data arguments and an optional idempotency key. It SHALL send the existing publish request without inventing envelope metadata or allowing a caller-selected schema version. Data SHALL support all JSON value kinds without coercing values or modifying the caller's input.

#### Scenario: JSON values retain their meaning
- **WHEN** a caller publishes an object, array, string, integer, finite floating-point number, boolean, or explicit null, including nested values and Unicode
- **THEN** the transmitted `data` SHALL preserve the JSON value and its value kind
- **AND** boolean values SHALL NOT become integers, integers SHALL NOT be coerced through floating-point representation, and null SHALL NOT be omitted

#### Scenario: Missing and explicit-null data differ
- **WHEN** a caller omits the required data argument
- **THEN** the SDK SHALL reject the call locally
- **AND WHEN** the caller explicitly supplies null
- **THEN** the SDK SHALL include `"data": null` and leave business-schema conformance to admission

#### Scenario: Invalid JSON input
- **WHEN** a payload includes a non-finite number, non-string object key, or unsupported Python object
- **THEN** the SDK SHALL reject it locally without stringifying it, sending invalid JSON, or making a network request

### Requirement: Python publish returns admission acceptance

A successful Python publish call SHALL return an acceptance receipt containing the server's queue request identifier and available message. It SHALL NOT report EventBridge delivery, subscriber receipt, or a client-generated event identifier as the outcome. Synchronous admission failures SHALL remain service errors.

#### Scenario: Valid publication accepted
- **WHEN** admission returns HTTP 202 with a nonempty `requestId`
- **THEN** the SDK SHALL return that identifier unchanged in an acceptance receipt
- **AND** it SHALL NOT wait for downstream event delivery

#### Scenario: Payload rejected by the schema
- **WHEN** admission returns HTTP 422 with payload violations
- **THEN** the SDK SHALL raise a service error preserving the violations
- **AND** it SHALL NOT return an acceptance receipt or retry the request

### Requirement: Python publication does not imply cross-request deduplication

The Python SDK SHALL forward a supplied idempotency key unchanged and omit it when absent, preserving server defaulting. It SHALL NOT automatically retry publication or describe a repeated key as duplicate suppression. Documentation SHALL distinguish retrying delivery of one admitted event from submitting another publish request.

#### Scenario: Supplied and omitted keys
- **WHEN** a caller supplies an idempotency key
- **THEN** the SDK SHALL send that key unchanged
- **AND WHEN** the caller omits it
- **THEN** the SDK SHALL leave key generation to admission rather than invent a new SDK default

#### Scenario: Caller deliberately resubmits
- **WHEN** the caller invokes publish again with the same key after losing a response
- **THEN** the SDK SHALL submit a new request only because the caller explicitly invoked it
- **AND** documentation SHALL explain that duplicate events and different admission-generated metadata remain possible
