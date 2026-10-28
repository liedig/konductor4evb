# konductor-sdk

Python client for the Konductor for Amazon EventBridge application APIs:
`publish`, `subscribe`, `unsubscribe`, and `describe_subscription`.

**Pre-alpha**: not published to a package index; the public surface may
change until the first stable release. Requires Python 3.11 or later.

## Install

```bash
pip install "git+https://github.com/liedig/konductor4evb.git@<revision>#subdirectory=sdks/python"
```

## Usage

```python
from konductor import KonductorClient

with KonductorClient(
    endpoint="https://{api-id}.execute-api.{region}.amazonaws.com/prod",
    token=access_token,  # Cognito access token
) as client:
    receipt = client.publish(
        domain="orders",
        event_name="OrderNotification",
        data={"orderId": "ord-123"},
    )
```

## Documentation

- [Python SDK guide](../../docs/guides/python-sdk.md) — installation, access
  tokens and `CognitoRefreshTokenProvider`, timeouts, results and errors,
  subscription lifecycle
- [SDK generation guide](../../docs/development/sdks.md) — contract
  ownership, regeneration, build and check commands

Support limits: application APIs only (no platform-administration
operations), synchronous only, no implicit retries.
