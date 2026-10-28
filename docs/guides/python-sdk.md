# Python SDK

[Documentation home](../README.md)

`konductor-sdk` is the supported Python client for the Konductor application
APIs. It wraps the four application operations behind a small synchronous
surface, attaches your Cognito access token to every request, and maps every
failure into a typed error.
The import namespace is `konductor`. Python 3.11 or later is required.

**Status: pre-alpha.** The SDK is not published to a package index. Its
public surface may change without notice until the first stable release.
Report security issues through the channels in
[SECURITY.md](https://github.com/liedig/konductor4evb/blob/main/SECURITY.md).

## Installation

Install from the repository at a pinned revision:

```bash
pip install "git+https://github.com/liedig/konductor4evb.git@<revision>#subdirectory=sdks/python"
```

or with [uv](https://docs.astral.sh/uv/):

```bash
uv add "konductor-sdk @ git+https://github.com/liedig/konductor4evb.git@<revision>#subdirectory=sdks/python"
```

There is no `konductor-sdk` package on PyPI; anything there under this name
is not this SDK.

## Quickstart: publish an event

```python
from konductor import KonductorClient

with KonductorClient(
    endpoint="https://{api-id}.execute-api.{region}.amazonaws.com/prod",
    token=access_token,
) as client:
    receipt = client.publish(
        domain="orders",
        event_name="OrderNotification",
        data={"orderId": "ord-123"},
    )
    print(receipt.request_id)
```

The endpoint's stage segment selects the environment (`/prod` above), exactly
as described in [choosing your environment](publishing-events.md#step-1-choose-your-environment-the-api-stage).
`publish` returns only when Konductor has validated and durably queued the
event; a returned receipt means HTTP `202 Accepted`. It says nothing about
delivery to subscribers.

`data` must be a JSON value: `None`, `bool`, `int`, finite `float`, `str`,
`list`, or `dict` with string keys. Anything else (sets, bytes, datetimes,
`NaN`) raises `TypeError` or `ValueError` before a request is sent. The
error names the location by array position only, such as `data.<key>[2]`,
because object keys are your payload and are never shown.
## Authentication

The SDK calls the OAuth API, which validates Cognito access tokens. Your
team groups in the token decide what you may publish or subscribe to; see
[Authenticate API requests](authentication.md) for how the platform checks
tokens and how to obtain one. Pass exactly one of `token` or
`token_provider` to `KonductorClient`; there is no default credential source.

### A fixed access token

```python
client = KonductorClient(endpoint=endpoint, token=access_token)
```

Suits short scripts. Access tokens expire after the platform's
`access_token_validity_minutes` (60 minutes by default); after that the
service answers `401` and the SDK raises `ServiceError`.

### Refreshed tokens from a Cognito refresh token

```python
from konductor import CognitoRefreshTokenProvider, KonductorClient

provider = CognitoRefreshTokenProvider(
    oauth_domain="https://{prefix}.auth.{region}.amazoncognito.com",  # CognitoOAuthDomain
    client_id="{client-id}",                                          # CognitoUserPoolClientId
    refresh_token=refresh_token,
)
client = KonductorClient(endpoint=endpoint, token_provider=provider)
```

Suits long-running processes. The provider exchanges the refresh token at
the Cognito token endpoint, reuses the access token until 60 seconds before
it expires (`refresh_margin`), then exchanges again. Konductor's app client
is public, so no client secret is involved. Get the refresh token by
signing in through the hosted UI ([Option A](authentication.md#option-a-hosted-ui-browser));
it stays valid for 30 days. If Cognito rotates the refresh token, the
provider keeps the new one in memory only. Persist a fresh refresh token
yourself if the process restarts.

### Your own token provider

`token_provider` accepts any zero-argument callable that returns an access
token. The client calls it before every request, so caching and refresh are
the provider's job. Use this to source tokens from a secret store, an SRP
sign-in ([Option B](authentication.md#option-b-srp-from-a-script-no-browser)),
or another identity provider that your deployment trusts.

### Failures and secrecy

An empty token, a token containing a line break, a provider that raises, or
a failed Cognito exchange raises `AuthenticationError` before any API
request is sent. A `401` from the service is never retried with a new token.
Tokens appear only in the `Authorization` header and never in exception
messages, tracebacks, logs, or representations. A provider's own exception
is not chained onto the `AuthenticationError`, so record anything you need
from it inside the provider.

## Endpoint guidance

- Use the `OAuthApiEndpoint` stack output. It ends in the `/dev` stage;
  swap the stage for `/staging` or `/prod` to target another environment. A
  trailing slash is accepted.
- The IAM API endpoint (`IamApiEndpoint`) does not accept access tokens.
- HTTPS is required; the client rejects `http`, URLs with credentials, query
  strings, or fragments at construction.
- The client sends no redirects: a `3xx` response surfaces as a
  `ServiceError` instead of being followed.
- Outbound proxies come from `HTTPS_PROXY` (or `ALL_PROXY`) and `NO_PROXY`.
  API requests and Cognito token exchanges use the same proxy selection.
- Proxies that require a login are not supported in this release. The SDK
  sends no `Proxy-Authorization` header, even when the proxy URL embeds
  `user:password@`, so such a proxy answers `407` and the request fails with
  a `TransportError`.

## Timeouts

`connect_timeout` defaults to 5 seconds and `read_timeout` to 30 seconds.
Both must be positive. Publish admission is synchronous validation plus a
queue write, so long reads indicate a platform problem; fail fast and let the
caller decide. A timed-out request raises `TransportError`; because the
response was lost, the event **may still have been accepted** — resubmission
is your decision. The SDK never retries implicitly.

## Results and errors

`publish` returns an `AcceptedCommand` with `request_id` (always present) and
`message` (optional service text).

Every failure is a subclass of `KonductorError`:

| Error | Meaning |
|---|---|
| `ConfigurationError` | Invalid client configuration; no request was sent |
| `AuthenticationError` | Credentials could not be resolved; no request was sent |
| `ServiceError` | The service answered with a non-success status |
| `TransportError` | The request failed or its response was lost; it may still have been accepted |
| `ProtocolError` | The response did not match the contract; acceptance is unknown |

`ServiceError` exposes `status`, `code`, `message`, `request_id`, `reason`,
and `field_violations` (validation failures as `(name, message)` pairs).
`request_id` comes from the error body, or from API Gateway's
`x-amzn-RequestId` response header when the body has none (gateway
authentication and throttling errors).
Because service messages can contain your payload content, they never appear
in `str()`, `repr()`, or tracebacks; read the attributes explicitly.

## Subscription lifecycle

Subscriptions move from `PENDING` to `ACTIVE`, and to `REMOVED` after
`unsubscribe`; failures land in `FAILED`. Individual environments can remain
`AWAITING_PROMOTION` while the overall subscription is `ACTIVE`.
See [Subscribe to events](subscriptions.md) for the full lifecycle.
Prerequisites — active consumer and producer domains, a registered schema,
bindings, and an entitlement — are platform-managed; the SDK neither
provisions them nor receives events. Consuming from your own queue or bus
remains your application's responsibility.

```python
receipt = client.subscribe(
    consumer_domain="checkout",
    producer_domain="orders",
    schema_name="acme-orders@OrderNotification",
    environments={"prod": False},  # optional: pause specific environments
)
```

`subscribe` and `unsubscribe` return `AcceptedCommand` receipts: the command
is queued, not completed. Authorization and rule materialization happen
asynchronously, and an accepted command can still fail. Unsubscribe takes
the full subscription identifier; dotted detail types are preserved:

```python
client.unsubscribe(subscription_id="checkout.orders.order.placed.v2")
```

### Inspecting state

`describe_subscription` reads current resource state:

```python
detail = client.describe_subscription(subscription_id="checkout.orders.OrderNotification")
print(detail.status)                      # e.g. ACTIVE, FAILED, REMOVED, or a future value
for name, env in detail.environments.items():
    print(name, env.status, env.pinned_version, env.error_detail)
```

This is an observed snapshot, **not** the outcome of a receipt you hold;
there is no correlation between the two, and the SDK provides no waiters or
polling. Partial activation stays visible: an environment can be
`AWAITING_PROMOTION` with no pinned version while another is `ACTIVE`.
Unknown status strings are preserved verbatim, and `error_detail` carries
recorded failure causes at the subscription and environment level. `404`
means the subscription does not exist and raises `ServiceError`; it is not
reported as a removed subscription.

## Examples

Runnable example scripts live in
[`sdks/python/examples/`](https://github.com/liedig/konductor4evb/tree/main/sdks/python/examples):
publishing with a fixed access token, publishing with tokens refreshed from
a Cognito refresh token, and subscribe, describe, publish, and unsubscribe
steps that you run separately, because subscription commands are processed
asynchronously. Their behavior is covered by mocked tests, so they
stay aligned with the SDK surface.

## Troubleshooting

- **`AuthenticationError` mentioning `invalid_grant`.** Cognito rejected the
  refresh token: it expired, was revoked, or belongs to another app client.
  Sign in again to get a new one, and check `client_id`.
- **`ServiceError` with status 401.** The access token is missing, expired,
  an ID token instead of an access token, or issued for another client. See
  [decoding a token](authentication.md#decoding-and-inspecting-a-token).
- **`ServiceError` with status 403.** You lack a publisher binding on the
  domain; see [publisher bindings](publishing-events.md#before-you-can-publish-publisher-bindings).
- **`ServiceError` with status 404.** The domain or schema does not exist in
  that environment — check the stage segment of your endpoint.
- **`ServiceError` with status 422.** Schema validation failed; read
  `message` for the schema violations.
- **`ServiceError` with status 3xx.** The endpoint URL is being redirected
  (for example a proxy or wrong hostname). The SDK does not follow redirects,
  because that could forward your access token elsewhere; fix the URL.
- **`TransportError`.** Network failure or timeout. The command may still
  have been accepted; resubmit only if your semantics tolerate a duplicate
  admission check.
