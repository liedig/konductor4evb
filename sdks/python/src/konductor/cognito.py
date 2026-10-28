"""Access tokens from a Cognito refresh token.

Konductor's user-pool app client is public, so the refresh-token grant needs
no client secret. The provider is a token_provider for KonductorClient: it
exchanges the refresh token at the hosted-domain token endpoint, caches the
access token, and exchanges again shortly before expiry.
"""

import json
import threading
import time
from collections.abc import Callable
from typing import Any
from urllib.parse import urlencode, urlsplit

import urllib3

from konductor import _config, _generated
from konductor._transport import DISABLED_RETRIES
from konductor.errors import AuthenticationError, ConfigurationError

DEFAULT_REFRESH_MARGIN = 60.0
DEFAULT_TOKEN_TIMEOUT = 10.0


def _token_url(oauth_domain: str) -> str:
    if not isinstance(oauth_domain, str) or not oauth_domain:
        raise ConfigurationError("oauth_domain must be a non-empty HTTPS URL")
    try:
        parts = urlsplit(oauth_domain)
        parts.port  # noqa: B018 - raises ValueError for an out-of-range port
    except ValueError:
        raise ConfigurationError("oauth_domain is not a valid URL") from None
    if parts.scheme != "https" or not parts.hostname:
        raise ConfigurationError("oauth_domain must be an HTTPS URL naming a host")
    if parts.username is not None or parts.password is not None:
        raise ConfigurationError("oauth_domain must not embed credentials")
    if parts.path.strip("/") or parts.query or parts.fragment:
        raise ConfigurationError(
            "oauth_domain must be the bare Cognito domain URL (CognitoOAuthDomain)"
        )
    return f"https://{parts.netloc}/oauth2/token"


def _non_empty(name: str, value: Any) -> str:
    if not isinstance(value, str) or not value:
        raise ConfigurationError(f"{name} must be a non-empty string")
    return value


def _token_endpoint_pool(token_url: str) -> Any:
    """Pool manager with the same proxy selection as the API transport
    (HTTPS_PROXY, ALL_PROXY, NO_PROXY), so both hosts take one network path."""
    configuration = _generated.Configuration(host=token_url, retries=DISABLED_RETRIES)
    return _generated.rest.RESTClientObject(configuration).pool_manager


class CognitoRefreshTokenProvider:
    """Token provider that exchanges a Cognito refresh token for access tokens.

    Pass it as KonductorClient(token_provider=...). oauth_domain is the
    CognitoOAuthDomain stack output and client_id the CognitoUserPoolClientId
    output. Failures raise AuthenticationError before any API request is
    sent; a failed exchange is attempted again on the next call.
    """

    def __init__(
        self,
        *,
        oauth_domain: str,
        client_id: str,
        refresh_token: str,
        refresh_margin: float = DEFAULT_REFRESH_MARGIN,
        timeout: float = DEFAULT_TOKEN_TIMEOUT,
    ) -> None:
        self._token_url = _token_url(oauth_domain)
        self._client_id = _non_empty("client_id", client_id)
        self._refresh_token = _non_empty("refresh_token", refresh_token)
        self._refresh_margin = _config.seconds(
            "refresh_margin", refresh_margin, allow_zero=True
        )
        self._timeout = _config.seconds("timeout", timeout)
        self._http: Any = _token_endpoint_pool(self._token_url)
        self._clock: Callable[[], float] = time.monotonic
        self._lock = threading.Lock()
        self._access_token: str | None = None
        self._expires_at = 0.0

    def __repr__(self) -> str:
        return (
            f"CognitoRefreshTokenProvider(token_url={self._token_url!r}, "
            f"client_id={self._client_id!r}, refresh_token=<hidden>)"
        )

    def __call__(self) -> str:
        with self._lock:
            now = self._clock()
            cached = self._access_token
            if cached is not None and now < self._expires_at - self._refresh_margin:
                return cached
            return self._exchange(now)

    def _exchange(self, now: float) -> str:
        form = urlencode(
            {
                "grant_type": "refresh_token",
                "client_id": self._client_id,
                "refresh_token": self._refresh_token,
            }
        )
        try:
            response = self._http.request(
                "POST",
                self._token_url,
                body=form,
                headers={
                    "Content-Type": "application/x-www-form-urlencoded",
                    "Accept": "application/json",
                },
                timeout=urllib3.Timeout(connect=self._timeout, read=self._timeout),
                retries=DISABLED_RETRIES,
                redirect=False,
            )
        except urllib3.exceptions.HTTPError as error:
            raise AuthenticationError(
                "the Cognito token endpoint could not be reached"
                f" ({type(error).__name__})"
            ) from None
        payload = _json_object(response.data)
        if response.status != 200:
            code = payload.get("error") if payload else None
            detail = f", error={code!r}" if isinstance(code, str) else ""
            raise AuthenticationError(
                f"Cognito rejected the refresh-token exchange"
                f" (status={response.status}{detail})"
            )
        access_token, expires_in = _issued_token(payload)
        rotated = payload.get("refresh_token") if payload else None
        if isinstance(rotated, str) and rotated:
            self._refresh_token = rotated
        self._access_token = access_token
        self._expires_at = now + expires_in
        return access_token


def _json_object(raw: bytes) -> dict[str, Any] | None:
    try:
        parsed = json.loads(raw)
    except (TypeError, ValueError):
        return None
    return parsed if isinstance(parsed, dict) else None


def _issued_token(payload: dict[str, Any] | None) -> tuple[str, int]:
    malformed = AuthenticationError("the Cognito token response is malformed")
    if payload is None:
        raise malformed
    access_token = payload.get("access_token")
    expires_in = payload.get("expires_in")
    token_type = payload.get("token_type", "Bearer")
    if not isinstance(access_token, str) or not access_token:
        raise malformed
    if isinstance(expires_in, bool) or not isinstance(expires_in, int):
        raise malformed
    if expires_in <= 0:
        raise malformed
    if not isinstance(token_type, str) or token_type.lower() != "bearer":
        raise malformed
    return access_token, expires_in
