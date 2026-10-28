# Python SDK from a checked OpenAPI projection

[Decision index](README.md)

- Recorded date: 2026-10-26.
- Change ID: `add-python-sdk`.
- Status: recorded design; generator compatibility gates passed.

## Decision and rationale

Konductor ships a Python SDK (`konductor-sdk`, import namespace `konductor`,
Python 3.11+) built in this repository on branch `feat-add-python-sdk`, with a
package lifecycle independent of the backend. The low-level client is
generated from a checked four-operation projection of the maintained OAuth
contract (`PublishEvent`, `Subscribe`, `Unsubscribe`,
`DescribeSubscription`); a small handwritten synchronous facade owns public
ergonomics, authentication, and policy.

## Authentication: OAuth access tokens only

The SDK calls only the OAuth (Cognito-authorized) API. Konductor is deployed
by organizations that bring their own authentication model; Cognito is the
current identity provider, and the OAuth API's REQUEST authorizer validates
its access tokens. `KonductorClient` takes exactly one of a fixed `token` or
a zero-argument `token_provider` called before every request. There is no
default credential source and no fallback between sources.

`CognitoRefreshTokenProvider` is the one bundled provider. It exchanges a
refresh token at the hosted-domain `/oauth2/token` endpoint, caches the
access token until a margin before expiry, and adopts a rotated refresh
token. It works with the platform's public app client, which has no secret.
A `401` is never retried with a fresh token.

An earlier draft of this change also offered IAM (SigV4) signing against the
IAM API. It was removed before merge: the SDK should not tie callers to AWS
credentials, and the platform IAM API (unchanged) stays a tool for AWS-native
automation. A client-credentials helper was considered and deferred, because
the stack has no confidential app client or resource server, the authorizer
accepts only the interactive client's `client_id`, and authorization comes
from `cognito:groups`, which machine tokens lack. Machine identities need a
platform change first.

The generator is OpenAPI Generator **7.26.0** (`python` generator, synchronous
`urllib3` library), pinned by URL with the Maven Central sha1
(`b96b148fcf482808037f68fbad99cb4d7e65d40b`) and a pin-time sha256
(`1760050094997b9cc790cc1350be095f15da8c08a996b184d3eadc4ccda5e35e`) recorded
in `sdks/codegen/openapi-generator-pin.json`. Both compatibility gates passed
against the stock configuration:

- **Serialization**: required explicit-null `data`, every JSON value kind,
  nested Unicode, booleans, integers above 2^53, optional omission, the
  composed subscription detail, nullable fields, and unknown output statuses
  all behave as specified (`sdks/python/tests/test_generation_conformance.py`).
  The generator's pydantic-based models keep `data` free-form and re-attach an
  explicitly set `None`, so `exclude_none` does not swallow JSON null.
- **Transport**: signing at the pool-manager boundary covers the final
  serialized bytes and URL; timeouts propagate; retries and redirects are
  disabled through `Configuration.retries`; connections are released through
  the adapter (`sdks/python/tests/test_transport_adapter.py`).

One named output-only projection normalization was required and is confined to
the projection (`sdks/codegen/extractor.py`,
`response-status-enums-as-strings`): response-side `status` enums become plain
strings so unfamiliar statuses decode instead of failing enum validation.
Request-side constraints are never touched.

## Source/projection boundary

The two OpenAPI files remain the only maintained contracts; the SDK reads
only `api/openapi-oauth.yaml`. `sdks/codegen/extractor.py` selects the four
operation IDs, collects their transitive local references, strips
authentication and API Gateway integration, and fails on missing operations
or dangling references. `make sdk-generate` regenerates the projection and the
committed client under `sdks/python/src/konductor/_generated/`;
`make sdk-generate-check` fails on stale artifacts or nondeterministic output
without touching the working tree. Generated source is committed so reviews
see actual client changes and consumers never need the generator or a JVM.

## Implementation gate outcomes (2026-10-26)

- `make sdk-check` (drift check, ruff 0.17.2 lint/format, mypy 2.4.0 strict):
  pass. `make sdk-test`: 152 SDK tests + 29 codegen tests, all mocked; pass
  on the development Python 3.14.8 with the lockfile resolved for the
  declared 3.11+ range (CI runs the 3.11/3.14 matrix). `make sdk-build`
  (uv 0.12.23): wheel and sdist build; the wheel installs into a clean
  virtual environment and the shipped examples run against the installed
  package. `openspec validate add-python-sdk --strict`: pass. Docs:
  `mkdocs build --strict` (mkdocs-material 9.7.7): pass.
- Generator: OpenAPI Generator 7.26.0, checksum-verified per
  `sdks/codegen/openapi-generator-pin.json`; two clean regenerations are
  byte-identical. Switching the source from the IAM to the OAuth contract
  changed only `projection.yaml`; the generated client is byte-identical.
- Live-stack smoke testing: explicitly skipped (not authorized for this
  change). No live AWS calls, no deployment, no package-index upload.
- Complete-diff review (task 6.2): findings resolved (drift check now ignores
  `__pycache__` bytecode artifacts; generation writes the projection only
  after a successful run). The diff contains no Rust or web-console changes,
  so `rust-reviewer` and `ui-reviewer` reviews are recorded as not
  applicable.

## Alternatives considered

- **Smithy-first generation**: deferred. The smithy-python clients are
  documented as unstable and asynchronous; a later migration is an explicit
  contract migration, not a side-by-side model.
- **Separate repositories per runtime**: rejected for now; contract changes
  would need coordinated PRs before one SDK has proved useful. Extraction
  remains an option.
- **Exposing the generated client directly**: rejected; generator upgrades
  would dictate public ergonomics, exception names, and model compatibility.
- **Custom generator or handwritten serialization**: rejected; both gates
  passed with stock configuration plus the small transport adapter, so no
  fork or template patch exists to maintain.

## Evidence and limits

Gate, extractor, drift-check, and pin evidence: `sdks/codegen/tests/`
(extractor, normalization, drift, checksum tests) and
`sdks/python/tests/` (conformance and transport). The contract discrepancy
that planning found in the subscription response schemas is recorded and
resolved as [D31](../reference/documentation-discrepancies.md#d31). Limits:
no async facade, no waiters, no automatic retries, no registry publishing,
and no live-stack smoke run is implied by this record. Package versioning is
independent of backend releases.
