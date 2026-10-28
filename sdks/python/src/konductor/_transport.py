"""Handwritten transport adapter around the generated client.

Signing happens at the pool-manager boundary so signatures cover the final
serialized body bytes and URL. Retries and redirects are disabled: commands
must not be replayed implicitly, and credentials must never follow a redirect.
"""

from collections.abc import Callable
from typing import Any

import urllib3

from konductor import _generated
from konductor.errors import ProtocolError

DEFAULT_CONNECT_TIMEOUT = 5.0
DEFAULT_READ_TIMEOUT = 30.0

Signer = Callable[..., dict[str, Any]]

DISABLED_RETRIES = urllib3.Retry(
    total=0,
    connect=0,
    read=0,
    redirect=0,
    status=0,
    raise_on_redirect=False,
    raise_on_status=False,
)


class SigningPoolManager:
    """Pool-manager wrapper that signs each request at the final boundary."""

    def __init__(self, wrapped: Any, sign: Signer) -> None:
        self.wrapped = wrapped
        self._sign = sign

    def request(
        self,
        method: str,
        url: str,
        body: Any = None,
        headers: dict[str, str] | None = None,
        **kwargs: Any,
    ) -> Any:
        signed_headers = self._sign(
            method=method, url=url, body=body, headers=dict(headers or {})
        )
        return self.wrapped.request(
            method, url, body=body, headers=signed_headers, **kwargs
        )

    def clear(self) -> None:
        self.wrapped.clear()


class TransportClient:
    def __init__(self, client: Any) -> None:
        self._client = client

    def __getattr__(self, name: str) -> Any:
        return getattr(self._client, name)

    def response_deserialize(
        self, response_data: Any, *args: Any, **kwargs: Any
    ) -> Any:
        """Report a 2xx body the generated models cannot decode as ProtocolError.

        Non-2xx responses raise ApiException from inside the generated call,
        so a ValueError (malformed JSON, contract type mismatch) escaping here
        always describes a success response whose outcome is unknown.
        """
        try:
            return self._client.response_deserialize(response_data, *args, **kwargs)
        except _generated.exceptions.OpenApiException:
            raise
        except ValueError:
            raise ProtocolError(
                f"the service returned HTTP {response_data.status} with a body that"
                " does not match the contract; the outcome is unknown"
            ) from None

    def close(self) -> None:
        close_client(self._client)


def build_client(host: str, sign: Signer) -> TransportClient:
    configuration = _generated.Configuration(host=host, retries=DISABLED_RETRIES)
    client = _generated.ApiClient(configuration)
    client.rest_client.pool_manager = SigningPoolManager(
        client.rest_client.pool_manager, sign
    )
    return TransportClient(client)


def close_client(client: Any) -> None:
    client.rest_client.pool_manager.clear()
