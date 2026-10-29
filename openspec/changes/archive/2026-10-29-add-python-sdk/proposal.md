# Proposal

## Why

Application teams and demo authors currently assemble HTTP requests, authentication, and response handling themselves to use Konductor's publish and subscription APIs. A dependable Python SDK will make those flows easy to demonstrate while establishing a shared contract and repeatable generation approach for later Rust, TypeScript, Java, and .NET SDKs.

## What Changes

- Add an independently versioned, installable Python SDK in this repository under `sdks/python/`, exposing synchronous `publish`, `subscribe`, `unsubscribe`, and `describe_subscription` operations.
- Call only the OAuth (Cognito-authorized) API endpoint. Each client takes exactly one of a fixed Cognito access token or a caller-supplied token provider, and the SDK bundles one provider that refreshes access tokens from a Cognito refresh token through the platform's public app client. Konductor is deployed by organizations with their own authentication model and Cognito is the current identity provider, so the SDK does not sign IAM (SigV4) requests or depend on AWS credentials. Do not implement interactive Cognito login or the client-credentials grant, which the platform cannot issue or authorize yet.
- Generate the low-level client from an automatically extracted four-operation OpenAPI subset, behind a small handwritten public interface. Evaluate a pinned OpenAPI Generator Python client against concrete compatibility tests before adopting it; do not make experimental Smithy tooling a prerequisite for this milestone.
- Preserve arbitrary JSON business data, structured errors, request identifiers, per-environment subscription states, and the difference between command acceptance and completed processing. Disable automatic command retries in the first release.
- Reconcile the selected OpenAPI response descriptions with existing handler behavior, including subscription failure details, awaiting-promotion state, and nullable response fields. Record the discrepancy and its evidence rather than changing server behavior to match incomplete documentation.
- Add shared wire-contract fixtures, reproducibility and package checks, and examples for fixed and refreshed access tokens. Document the application-team workflow and the future generator-replacement boundary.
- Carry out this change on `feat-add-python-sdk`. This proposal creates planning artifacts only; implementation requires a separate apply request.

## Capabilities

### New Capabilities

None. SDK access extends existing publishing, subscription, access, and contributor-validation capabilities rather than introducing another business bounded context.

### Modified Capabilities

- `control-plane-access`: Installable Python client boundary, explicit access-token authentication with Cognito refresh, credential safety, transport configuration, and consistent error handling.
- `event-publishing`: Python publication of arbitrary JSON with faithful acceptance receipts and explicit duplicate-delivery limitations.
- `event-subscriptions`: Python subscribe, unsubscribe, and subscription inspection without conflating acceptance with activation or event consumption.
- `ci-pipeline`: Reproducible SDK contract extraction, generation, tests, and package validation using shared local/CI checks without live credentials.

## Impact

- New SDK package, generation tooling, language-neutral conformance fixtures, and tests under `sdks/`; focused additions to the Makefile and existing CI workflow.
- Descriptive corrections to the covered response schemas in `api/openapi-iam.yaml` and `api/openapi-oauth.yaml`. Route integrations, request validation, authorization, Lambda handlers, resources, and deployment topology remain unchanged.
- Build-time generator tooling and Python development dependencies; runtime HTTP/model dependencies only, with no AWS SDK. Package consumers will not need the code generator or a JVM.
- An application-team SDK guide, contributor generation/testing guidance, links from API/authentication references, a decision record and index entry, discrepancy evidence, and an Unreleased changelog entry.
- No platform APIs, schema administration, subscription state/list APIs, receive loop, automatic waiters, async Python facade, other-language packages, Smithy migration, public registry upload, release automation, or live provisioning in this change. Examples use preconfigured domains, schemas, bindings, and entitlements.
