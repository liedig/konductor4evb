# Spec Delta

## ADDED Requirements

### Requirement: Installable application-facing Python client

Konductor SHALL provide an independently versioned Python package with synchronous publish, subscribe, unsubscribe, and subscription-detail operations. Installing a built package SHALL NOT require the repository, code-generation tools, or a JVM. The supported public interface SHALL exclude platform administration.

#### Scenario: Install a built distribution
- **WHEN** a user installs the SDK wheel into a clean supported Python environment
- **THEN** the four operations SHALL be importable through the documented public interface
- **AND** the installation SHALL NOT run code generation or require a Konductor checkout

#### Scenario: Close a client
- **WHEN** the user closes the client or exits its context manager
- **THEN** the SDK SHALL release its owned HTTP resources

### Requirement: Explicit Python SDK access-token authentication

Each Python client SHALL target an explicit OAuth API endpoint and SHALL take exactly one token source: a fixed access token or a synchronous token provider. The SDK SHALL NOT read ambient credentials, sign IAM (SigV4) requests, switch endpoints or token sources after an authentication failure, or start an interactive login.

#### Scenario: Token source is missing or ambiguous
- **WHEN** a client is configured with no token source, both a token and a provider, an empty token, or a non-callable provider
- **THEN** the SDK SHALL report a configuration error before sending any request

#### Scenario: Service rejects the token
- **WHEN** the service answers an operation with 401
- **THEN** the SDK SHALL report the service error after exactly one request
- **AND** it SHALL NOT call the token provider again to retry the operation

#### Scenario: No platform-administration credentials needed
- **WHEN** a user with only team-group access constructs and uses a client
- **THEN** the four application operations SHALL be available without platform-administration roles

### Requirement: Bearer tokens and Cognito refresh

The Python SDK SHALL resolve a token provider before each request and attach the token only to the Authorization header. The SDK SHALL provide a Cognito refresh-token provider that exchanges a refresh token for an access token at the user pool's hosted-domain token endpoint using the public app client, without a client secret. The provider SHALL reuse an access token until a configurable margin before its expiry and SHALL adopt a rotated refresh token when Cognito returns one.

#### Scenario: Caller refreshes a token
- **WHEN** the provider returns a different token on the next operation
- **THEN** the SDK SHALL use that token without rebuilding the client

#### Scenario: Token provider fails
- **WHEN** the provider raises an error or supplies an empty token
- **THEN** the SDK SHALL report an authentication error without making an unauthenticated request or starting a login flow

#### Scenario: Cached access token is reused
- **WHEN** consecutive operations run before the cached access token reaches its refresh margin
- **THEN** the Cognito provider SHALL make no further token-endpoint request

#### Scenario: Refresh token is rejected
- **WHEN** the token endpoint rejects the exchange, is unreachable, or returns a malformed token response
- **THEN** the SDK SHALL report an authentication error naming any OAuth error code
- **AND** the error SHALL NOT contain the refresh token or an access token
- **AND** no API request SHALL be sent

### Requirement: Bounded and credential-safe SDK transport

The Python SDK SHALL verify TLS, require HTTPS endpoints, preserve configured base-path prefixes, and expose finite configurable connect and read timeouts. It SHALL NOT follow redirects or automatically retry requests in its first release. SDK-owned logs and object representations SHALL exclude credentials, tokens, and business payloads.

#### Scenario: Endpoint includes an environment stage
- **WHEN** the configured endpoint ends in `/prod`, with or without a trailing slash
- **THEN** publication SHALL target that prefix followed by `/publish`, not a root-level path or duplicated stage

#### Scenario: Unsafe endpoint or redirect
- **WHEN** configuration includes a non-HTTPS endpoint, URL credentials, a query, or a fragment, or the server returns a redirect
- **THEN** the SDK SHALL reject the configuration or report the response without forwarding credentials to another destination

#### Scenario: Timeout is not retried silently
- **WHEN** an operation times out or loses its response
- **THEN** the SDK SHALL report a transport error after the configured transport limits
- **AND** it SHALL make no automatic second attempt
- **AND** it SHALL NOT assert that the service did not accept a command

### Requirement: Consistent Python SDK error information

The Python SDK SHALL distinguish service, transport, configuration, and malformed-response failures. Service errors SHALL expose HTTP status, available service code, message, request identifier, and field violations. Unrecognized or non-JSON service errors SHALL still produce a usable error without requiring the documented error body to be present.

#### Scenario: Structured validation failure
- **WHEN** the service rejects a request with a JSON error containing field violations
- **THEN** the SDK SHALL expose the status, code, message, and violations without replacing them with a generic serialization exception

#### Scenario: Gateway error lacks modeled fields
- **WHEN** API Gateway returns an error without `code`, or an intermediary returns a non-JSON error
- **THEN** the SDK SHALL preserve the HTTP status and any safe available request identifier
- **AND** it SHALL report the service failure without exposing raw response bodies in automatic logs or exception representations

#### Scenario: Acceptance response is malformed
- **WHEN** a command returns HTTP 202 without a nonempty `requestId`
- **THEN** the SDK SHALL report a malformed-response error rather than a valid acceptance receipt
- **AND** it SHALL NOT retry or claim that the command was not queued

### Requirement: Secret-free Python SDK failures

Python SDK failures SHALL NOT expose tokens, refresh tokens, payload object keys, or service response content in rendered tracebacks, including chained exceptions, in addition to messages and representations. Explicitly accessible service error attributes SHALL remain available.

#### Scenario: Token contains a line break
- **WHEN** a fixed token or a provider's token contains a carriage return or line feed
- **THEN** the SDK SHALL report an authentication error before building the request
- **AND** neither the error nor its rendered traceback SHALL contain the token

#### Scenario: Token provider raises with secret text
- **WHEN** a custom token provider raises an exception whose message contains token material
- **THEN** the SDK SHALL report an authentication error whose rendered traceback does not include the provider's exception or its message

#### Scenario: Service error body contains payload data
- **WHEN** the service rejects a request with an error body that echoes caller payload values
- **THEN** the rendered traceback of the resulting service error SHALL NOT contain those values
- **AND** the message and field violations SHALL remain readable through explicit attributes

#### Scenario: Invalid payload has a secret-bearing key
- **WHEN** a caller publishes data that fails local JSON validation and an object key on the path carries sensitive text
- **THEN** the SDK SHALL reject the data before sending a request
- **AND** neither the error nor its rendered traceback SHALL contain that key

#### Scenario: Every failure path is checked
- **WHEN** the SDK test suite drives each failure path with a marker secret
- **THEN** the marker SHALL be absent from the full rendered traceback of every resulting error
