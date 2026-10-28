"""Bearer-token request signing for the Konductor client."""

from collections.abc import Callable
from typing import Any

from konductor._transport import Signer
from konductor.errors import AuthenticationError, ConfigurationError, KonductorError

TokenProvider = Callable[[], str]


def bearer_signer(
    *, token: str | None = None, token_provider: TokenProvider | None = None
) -> Signer:
    """Validate the token source and return a signer that attaches it.

    A fixed token is sent unchanged; a provider is called before every
    request so the caller (or CognitoRefreshTokenProvider) owns refresh.
    """
    if (token is None) == (token_provider is None):
        raise ConfigurationError("pass exactly one of token or token_provider")
    if token_provider is None:
        if not isinstance(token, str) or not token:
            raise ConfigurationError("token must be a non-empty string")
        fixed = _header_safe(token, "token")

        def resolve() -> str:
            return fixed

    else:
        if not callable(token_provider):
            raise ConfigurationError("token_provider must be callable")
        provider = token_provider

        def resolve() -> str:
            return _call_provider(provider)

    def sign(
        method: str, url: str, body: Any, headers: dict[str, str]
    ) -> dict[str, str]:
        headers["Authorization"] = f"Bearer {resolve()}"
        return headers

    return sign


def _call_provider(provider: TokenProvider) -> str:
    try:
        token = provider()
    except KonductorError:
        raise
    except Exception:
        raise AuthenticationError("token provider failed") from None
    if not isinstance(token, str) or not token:
        raise AuthenticationError("token provider returned an empty token")
    return _header_safe(token, "token provider result")


def _header_safe(token: str, source: str) -> str:
    # The HTTP layer echoes the whole header value, secret included, when it
    # rejects a line break, so reject one before the header is built.
    if "\r" in token or "\n" in token:
        raise AuthenticationError(f"{source} must not contain line breaks")
    return token
