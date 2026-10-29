# Design

## Context

See [proposal.md](proposal.md) for motivation and scope. Exploration selected dependable Python demos over proving experimental Smithy tooling, and confirmed access-token authentication against the OAuth API and subscription inspection. An earlier revision also planned IAM (SigV4) signing; it was removed during implementation because Konductor deployments bring their own authentication model, with Cognito as the current identity provider.

The existing contracts expose `PublishEvent`, `Subscribe`, `Unsubscribe`, and `DescribeSubscription` on both surfaces. Publish admits synchronously before returning 202; the subscription commands queue before resource authorization. Detail is current resource state, not a command-result endpoint. `docs/reference/api.md` identifies the two OpenAPI files as the maintained inventories.

There is no existing Konductor SDK package or generation pipeline. Python script tests live under `scripts/tests`; `.github/workflows/ci.yml` has Rust and web gates with read-only permissions. The SDK needs its own Python tooling without moving those existing tests.

### Contract discrepancy discovered during planning

Both OpenAPI files omit `errorDetail` from subscription summaries and environments, omit `ruleName` from environments, and omit `AWAITING_PROMOTION` from the environment status enum. `SubscriptionDetail.removedAt` is described as a non-null string, but `describe-subscription/src/handler.rs` emits null before removal; its top-level `errorDetail` can also be null. `lib/konductor-common/src/streams.rs` serializes the per-environment fields with absent optional values and includes awaiting promotion.

The event-subscriptions main spec requires awaiting-promotion and failure details. Existing evidence includes `awaiting_promotion_env_round_trips_without_materialization` and `failed_env_carries_error_detail` in `streams.rs`. The implementation task must record this discrepancy in `docs/reference/documentation-discrepancies.md`, then reconcile only the affected descriptive response schemas against handler/model evidence. Do not conceal it in generator-specific patches or change server behavior to match the incomplete schema.

## Goals / Non-Goals

**Goals:**
- Keep one maintained API contract per existing surface and generate the SDK from one checked projection of the OAuth contract.
- Make public Python ergonomics independent of generator-specific classes.
- Establish wire-level tests that later runtimes and generators can reuse.
- Produce a locally installable, independently versioned package without live AWS access in normal tests.

**Non-Goals:**
- Changing Lambda behavior, admission rules, queue semantics, domain authorization, or resource topology.
- Moving SDK development to another repository, sharing backend release numbers, or generating all application APIs immediately.
- Creating a custom generator, permanent generator fork, or Smithy model alongside the existing contracts in this milestone.
- Publishing to a registry, provisioning demo infrastructure, or promising production maturity solely because a generator is labeled stable.

## Decisions

### 1. Develop in this repository with a separate package lifecycle

Use `feat-add-python-sdk`, branched from the current local main, which already contains the two earlier maintenance commits. Do not rewrite or move those commits. SDK versioning is independent of the backend; the initial distribution name is `konductor-sdk`, import namespace `konductor`, and minimum Python version 3.11. These are proposed package defaults, not a claim that a registry name is reserved.

```text
api/openapi-oauth.yaml --> checked four-operation projection
                                         |
                                         v
sdks/codegen/ --------------------> pinned generator
                                         |
                                         v
sdks/python/src/konductor/_generated/
                                         |
                                         v
sdks/python/src/konductor/        public interface and auth adapter

sdks/conformance/                shared JSON request/response fixtures
sdks/python/tests/               Python behavior and package tests
sdks/python/examples/            explicit, operator-run examples
```

Keep generated source committed so reviews can see the actual client changes and consumers can build an sdist without a generator. Regeneration checks work in temporary directories and never clean or overwrite unrelated work. A future SDK-only repository is an extraction option, not a present dependency.

Alternative: separate repositories per runtime. Rejected now because contract changes would require coordinated PRs before one SDK has proved useful.

### 2. Use an OpenAPI projection with an explicit generator compatibility gate

Implement a deterministic extractor in `sdks/codegen/` that selects the four operation IDs and their transitive local schema/parameter/response dependencies from `api/openapi-oauth.yaml`, then strips authentication (`security`) and AWS deployment integration (`x-amazon-apigateway-*`, `servers`). Fail on unknown references or missing operations. Retain wire constraints and field names; do not copy unrelated routes or deployment-specific endpoint values. The IAM contract is not an SDK input, so IAM-only changes never regenerate the SDK.

Authentication is supplied by the public client's token source, not inferred from API Gateway vendor extensions. Any output-only normalization, such as representing response status enums as strings for forward compatibility, must be named, tested, and confined to the projection. It must not weaken input validation, hide surface drift, or invent response fields absent from the maintained contracts.

The chosen first candidate is [OpenAPI Generator's Python generator](https://openapi-generator.tech/docs/generators/python/) with the synchronous `urllib3` library. Before building the public SDK around it, pin an exact release and checksum and prove:

1. Required `data` is not lost when null, and nested JSON, booleans, integers above 2^53, finite decimals represented as Python floats, and Unicode serialize without coercion.
2. Detail's composed schemas decode absent and nullable fields, failure details, awaiting environments, additional fields, and unfamiliar status strings.
3. A supported transport extension can sign the actual serialized request, disable retries and redirects, set timeouts, and release connections without editing generated source.
4. Repeated generation is deterministic, and the generated dependency set supports the declared Python versions.

This is a bounded feasibility gate, not an unresolved choice between multiple planned implementations. If a stock configuration and small adapter cannot pass, stop implementation at the gate and revise this design with the user. Do not silently adopt a custom generator, manually implement all serialization, restrict supported JSON values, or switch to experimental Smithy.

Pin the successful generator invocation, configuration, and input provenance. Keep build/development versions reproducible with a `uv` lockfile while declaring suitable runtime dependency bounds for package consumers.

Alternative: Smithy-first generation. Deferred because the [current Python generator documentation](https://github.com/smithy-lang/smithy-python) describes unstable, asynchronous clients and building generator libraries locally. Smithy can later replace the source/projection pipeline, but that requires an explicit contract migration rather than a second unchecked model.

### 3. Keep the handwritten public layer small

Expose `KonductorClient` (taking exactly one of `token` or `token_provider`), `CognitoRefreshTokenProvider`, acceptance receipts, subscription-detail result types, and a small SDK error hierarchy. Methods are keyword-oriented synchronous calls named in the proposal. Do not export generated model classes as the supported public API.

The facade maps snake_case arguments to generated wire models and delegates HTTP serialization. It validates JSON compatibility without mutating caller data or applying the business schema locally. Use a required argument for `data`; explicit `None` is JSON null, not an omitted value. Optional parameters need an omission sentinel where omission and null differ. Never serialize Python objects through `str()` as a fallback.

The three commands return an `AcceptedCommand` carrying `request_id` and the optional server message. Require a nonempty request ID even though the current OpenAPI response does not mark it required. Report malformed success bodies as protocol errors with acceptance unknown.

Detail results preserve known fields and nullable/absent values. Represent statuses as forward-compatible strings rather than closed generated enums; unrecognized fields can be ignored, but unknown statuses must not be mapped to ACTIVE. Do not add a waiter or derive readiness from acceptance.

Alternative: expose generated clients directly. Rejected because generator upgrades would dictate demo ergonomics, exception names, and model compatibility. Avoid a second generic HTTP framework: this facade owns only Konductor's four operations and cross-cutting policy.

### 4. Authenticate at the final pre-send boundary

Each client has one immutable endpoint and token source. Validate an HTTPS URL without embedded credentials, query, or fragment and retain its base path. Do not infer stage from hostnames, discover stack outputs, or permit caller auth-header overrides.

- Token source: accept exactly one fixed token or synchronous provider. The client resolves the provider before each request and caches nothing itself. Do not decode tokens to authorize locally, retry a 401, or start Cognito sign-in. A provider's own `AuthenticationError` passes through unchanged; any other provider failure becomes an `AuthenticationError`.
- Cognito refresh: `CognitoRefreshTokenProvider` posts the `refresh_token` grant with the public app client ID (no secret) to `{CognitoOAuthDomain}/oauth2/token`, with finite timeouts and retries and redirects disabled. It caches the access token until `refresh_margin` (default 60 seconds) before `expires_in`, adopts a rotated refresh token, serializes refresh under a lock, and reports rejections with the status and OAuth error code only. A failed exchange caches nothing, so the next call tries again.
- Both: fail before sending when no usable token is available. Disable redirects, including same-host redirects, to avoid forwarding tokens. Do not log request bodies, tokens, or raw response bodies; exception representations use safe summaries while explicit attributes retain useful service error information.
- Secrecy extends to rendered tracebacks, not only `str()` and `repr()`. An SDK error that replaces a failure which may carry a token, a provider's own exception, or a service body suppresses the chained cause (`raise ... from None`). Reject a token containing CR or LF with an `AuthenticationError` before building the header, because the HTTP layer otherwise echoes the full `Bearer` value in its own error. Enforce this with one leak test rather than per-path fixes: drive every failure path (fixed and provider tokens with line breaks, a provider raising with token text, Cognito failures, 4xx/5xx service errors, undecodable success bodies, transport errors) with a marker secret and assert the marker never appears in `traceback.format_exception` output.
- Proxies: the API transport and the Cognito token exchange select a proxy identically from `HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY` through the generated configuration. Proxy URLs with embedded credentials are not supported in this release: the SDK sends no `Proxy-Authorization`, so an authenticating proxy answers 407. Supporting them would add a second secret-handling path, so it is deferred and the guide states the limitation.
- Configuration parsing never leaks raw library errors: an unparseable endpoint or Cognito domain raises `ConfigurationError`.

Tests use injected providers, a recording transport, and a recording token endpoint, never live identity services.

Alternative: IAM (SigV4) signing against the IAM API. Removed during implementation: the SDK should not tie callers to AWS credentials, and the IAM API remains a platform tool for AWS-native automation. Alternative: a client-credentials provider. Deferred because the stack has no confidential app client or resource server, the OAuth authorizer accepts only the interactive client's `client_id`, and authorization derives from `cognito:groups`, which machine tokens lack; machine identities need a platform change first.

### 5. Prefer explicit retries over hidden duplicates

Use finite connect/read timeouts (proposed defaults: 5 and 30 seconds) and disable automatic retries at both generated-client and HTTP-library layers for all four operations initially. These are transport limits, not an end-to-end processing deadline. Expose configurable positive timeout values.

Forward a caller's publish idempotency key exactly, omitting it when absent. Do not apply Smithy's idempotency-token semantics or generate an SDK key: the existing service uses the key as event metadata, not as a cross-request deduplication store. A lost response or malformed acceptance means the command may have been accepted. User code decides whether to resubmit.

Map non-success statuses into `KonductorServiceError` attributes for status, code, message, request identifier, reason, and field violations where supplied. Use separate transport, configuration, and protocol error categories. Do not require all gateway errors to match `ErrorResponse`; allow safe generic summaries for non-JSON errors. Service messages and field violations are explicitly accessible but excluded from automatic exception representations that could leak payload content.

When the body omits a request identifier, fall back to API Gateway's `x-amzn-RequestId` response header, because gateway authentication and throttling errors carry it only there. Treat a `fieldList` that is not an array as absent, so a malformed error body still yields a `ServiceError` with its status. A 2xx body that is not valid JSON or does not match the contract is a protocol error with the outcome unknown. Publish schema rejections currently carry their violations in `message` with no `fieldList`; document that, and record it as a discrepancy if platform documentation says otherwise.

Alternative: default exponential-backoff retries. Deferred until each operation's replay semantics and retryable failures have an explicit contract; the name `idempotencyKey` is not sufficient evidence.

### 6. Separate repeatable tests from live demo execution

Shared conformance fixtures contain synthetic request/response JSON, not Python classes. Exercise them through the public facade with a recording transport. Add Python-specific assertions for token attachment and rotation, Cognito refresh, strict JSON conversion, timeouts, no retries, redirects, safe representations, and connection cleanup.

Provide `make sdk-generate`, `make sdk-check`, `make sdk-test`, and `make sdk-build` as proposed local entry points, backed by the same locked tools CI uses. Every check, including the codegen suite and the drift check, runs through the SDK project with `uv run --locked`, so a stale lock fails instead of being repaired and test-tool releases cannot change the gate. Use an independent SDK job in the existing CI workflow without altering its trigger or permission policy. Validate Python 3.11 and 3.14 initially, generated-source drift, formatting/type checks, unit/conformance tests, sdist-to-wheel builds, and clean wheel imports. Do not invoke full backend builds from an SDK-only local target.

Examples pass a fixed access token or a Cognito refresh-token provider explicitly and separate command submission from inspection. They use operator-provided endpoints and preconfigured resources. Mocked example tests are mandatory. A real-stack smoke run is optional, requires separate explicit authorization with the environment and resources named, and must report skipped/unavailable status honestly. Neither planning nor apply authorization permits a registry upload or live write by itself.

### 7. Preserve handler durability boundaries

No new or changed handler write path exists in this change. The six-question handler side-effects rubric therefore has no new path to classify and needs no invented per-handler record. SDK HTTP requests continue to use the existing publish admission and envelope-driven subscription command boundaries; no SDK code directly commits DynamoDB writes or performs follow-on EventBridge, SQS, or rule mutations.

If implementation discovers a need for server behavior changes, stop and amend scope and durability analysis rather than hiding them in this SDK change.

## Risks / Trade-offs

- Generator support tables do not guarantee JSON null or composed-schema behavior. Mitigation: make the compatibility gate the first executable SDK work, with unweakened fixtures and a stop condition.
- Incomplete OpenAPI response models can produce a misleading SDK. Mitigation: record and reconcile the observed discrepancy using existing handler/model evidence before final projection generation.
- Access tokens expire mid-process. Mitigation: resolve the token source per request and offer a refresh-token provider that renews before expiry, with no redirects or hidden retries.
- Refresh tokens are long-lived secrets. Mitigation: keep them out of URLs, logs, errors, and representations; rotated refresh tokens live only in memory.
- Secrecy guarantees erode one exception path at a time (review found chained generated exceptions, chained provider exceptions, and HTTP-layer header errors in successive rounds). Mitigation: a single leak test over every failure path, rendered through full tracebacks.
- Callers behind authenticating proxies cannot use the SDK until proxy credentials are supported. Mitigation: document the limitation; credentialed proxies are a follow-up with their own secrecy coverage.
- No retries or waiters requires a few more demo steps. Mitigation: make acceptance and inspection explicit rather than offer an incorrect success guarantee.
- Shared repository growth can add tooling overhead. Mitigation: isolate SDK dependencies, checks, and versions; consumers install built packages and never run the generator.
- Generator replacement still costs work. Mitigation: preserve public behavior and shared fixtures; do not promise zero-cost migration or identical language idioms.

## Migration Plan

1. Keep implementation on `feat-add-python-sdk` and record the API discrepancy.
2. Reconcile descriptive schemas, establish extraction tests, and pass the pinned-generator feasibility gate.
3. Build the public interface and auth adapter test-first, then wire package checks and mocked examples into CI.
4. Add `docs/guides/python-sdk.md` as the primary user guide and `docs/development/sdks.md` for generation/maintenance; update navigation, API/authentication links, testing guidance, a decision record and index, and the Unreleased changelog.
5. Produce local distributions. No deployment, public publishing, or release workflow is needed. A later authorized release must verify registry name availability.

Rollback is stopping use of the new package or reverting the SDK-only changes; no persisted server state or resource migration is introduced. Keep evidence-based descriptive OpenAPI corrections independently reviewable.
