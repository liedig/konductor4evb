import json
import logging
from urllib.parse import parse_qs

import pytest
import urllib3
from conftest import RecordingTransport

import konductor
from konductor import CognitoRefreshTokenProvider

DOMAIN = "https://auth.example.invalid"
CLIENT_ID = "client-abc"
REFRESH = "refresh-secret-xyz"


class TokenEndpoint:
    """Records token requests and replies with queued responses."""

    def __init__(self, *responses):
        self.responses = list(responses)
        self.requests = []

    def request(self, method, url, body=None, headers=None, **kwargs):
        self.requests.append(
            {
                "method": method,
                "url": url,
                "form": {k: v[0] for k, v in parse_qs(body or "").items()},
                "headers": headers or {},
                "kwargs": kwargs,
            }
        )
        response = self.responses.pop(0)
        if isinstance(response, Exception):
            raise response
        status, payload = response
        raw = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
        return urllib3.HTTPResponse(
            body=raw,
            status=status,
            headers={"Content-Type": "application/json"},
            preload_content=True,
        )


def issued(access="access-1", expires_in=3600, **extra):
    return (200, {"access_token": access, "expires_in": expires_in, **extra})


class Clock:
    def __init__(self):
        self.now = 1000.0

    def __call__(self):
        return self.now


def provider_with(endpoint, clock=None, **overrides):
    options = {
        "oauth_domain": DOMAIN,
        "client_id": CLIENT_ID,
        "refresh_token": REFRESH,
        **overrides,
    }
    provider = CognitoRefreshTokenProvider(**options)
    provider._http = endpoint
    provider._clock = clock or Clock()
    return provider


def test_exchanges_the_refresh_token_at_the_cognito_token_endpoint():
    endpoint = TokenEndpoint(issued("access-1"))
    assert provider_with(endpoint)() == "access-1"
    request = endpoint.requests[0]
    assert request["method"] == "POST"
    assert request["url"] == f"{DOMAIN}/oauth2/token"
    assert request["form"] == {
        "grant_type": "refresh_token",
        "client_id": CLIENT_ID,
        "refresh_token": REFRESH,
    }
    assert request["headers"]["Content-Type"] == "application/x-www-form-urlencoded"
    assert "Authorization" not in request["headers"]


def test_trailing_slash_on_the_domain_is_ignored():
    endpoint = TokenEndpoint(issued())
    provider_with(endpoint, oauth_domain=DOMAIN + "/")()
    assert endpoint.requests[0]["url"] == f"{DOMAIN}/oauth2/token"


def test_access_token_is_cached_until_the_refresh_margin():
    clock = Clock()
    endpoint = TokenEndpoint(issued("access-1", 3600), issued("access-2", 3600))
    provider = provider_with(endpoint, clock=clock, refresh_margin=60)
    assert provider() == "access-1"
    clock.now += 3600 - 61
    assert provider() == "access-1"
    assert len(endpoint.requests) == 1
    clock.now += 2
    assert provider() == "access-2"
    assert len(endpoint.requests) == 2


def test_rotated_refresh_token_is_used_for_the_next_exchange():
    clock = Clock()
    endpoint = TokenEndpoint(
        issued("access-1", 120, refresh_token="refresh-rotated"), issued("access-2")
    )
    provider = provider_with(endpoint, clock=clock, refresh_margin=60)
    provider()
    clock.now += 61
    provider()
    assert endpoint.requests[1]["form"]["refresh_token"] == "refresh-rotated"


def test_rejected_refresh_token_reports_the_oauth_error_without_secrets():
    endpoint = TokenEndpoint(
        (
            400,
            {
                "error": "invalid_grant",
                "error_description": "Refresh Token has expired",
            },
        )
    )
    with pytest.raises(konductor.AuthenticationError) as raised:
        provider_with(endpoint)()
    message = str(raised.value)
    assert "invalid_grant" in message
    assert "400" in message
    assert REFRESH not in message
    assert REFRESH not in repr(raised.value)


@pytest.mark.parametrize(
    "response",
    [
        (200, b"not json"),
        (200, ["not", "an", "object"]),
        (200, {"expires_in": 3600}),
        (200, {"access_token": "", "expires_in": 3600}),
        (200, {"access_token": "a", "expires_in": 0}),
        (200, {"access_token": "a", "expires_in": True}),
        (200, {"access_token": "a", "expires_in": "3600"}),
        (200, {"access_token": "a"}),
        (200, {"access_token": "a", "expires_in": 3600, "token_type": "mac"}),
        (302, {}),
    ],
    ids=[
        "non-json",
        "non-object",
        "no-access-token",
        "empty-access-token",
        "zero-expiry",
        "bool-expiry",
        "string-expiry",
        "no-expiry",
        "wrong-token-type",
        "redirect",
    ],
)
def test_unusable_token_responses_are_authentication_errors(response):
    endpoint = TokenEndpoint(response)
    with pytest.raises(konductor.AuthenticationError):
        provider_with(endpoint)()
    assert len(endpoint.requests) == 1


def test_unreachable_endpoint_is_an_authentication_error_without_retry():
    endpoint = TokenEndpoint(urllib3.exceptions.NewConnectionError(None, "refused"))
    with pytest.raises(konductor.AuthenticationError):
        provider_with(endpoint)()
    assert len(endpoint.requests) == 1


def test_failed_exchange_is_attempted_again_on_the_next_call():
    endpoint = TokenEndpoint((500, {}), issued("access-after-outage"))
    provider = provider_with(endpoint)
    with pytest.raises(konductor.AuthenticationError):
        provider()
    assert provider() == "access-after-outage"


def test_token_requests_disable_redirects_and_retries_and_set_timeouts():
    endpoint = TokenEndpoint(issued())
    provider_with(endpoint, timeout=4.5)()
    kwargs = endpoint.requests[0]["kwargs"]
    assert kwargs["redirect"] is False
    assert kwargs["retries"].total == 0
    assert kwargs["timeout"].connect_timeout == 4.5
    assert kwargs["timeout"].read_timeout == 4.5


@pytest.mark.parametrize(
    "overrides",
    [
        {"oauth_domain": "http://auth.example.invalid"},
        {"oauth_domain": "auth.example.invalid"},
        {"oauth_domain": "https://user:pw@auth.example.invalid"},
        {"oauth_domain": "https://auth.example.invalid/oauth2/token"},
        {"oauth_domain": "https://auth.example.invalid?x=1"},
        {"oauth_domain": ""},
        {"client_id": ""},
        {"refresh_token": ""},
        {"refresh_token": None},
        {"refresh_margin": -1},
        {"refresh_margin": float("nan")},
        {"timeout": 0},
        {"timeout": True},
        {"timeout": 10**400},
        {"refresh_margin": 10**400},
    ],
)
def test_invalid_configuration_is_rejected(overrides):
    options = {
        "oauth_domain": DOMAIN,
        "client_id": CLIENT_ID,
        "refresh_token": REFRESH,
        **overrides,
    }
    with pytest.raises(konductor.ConfigurationError):
        CognitoRefreshTokenProvider(**options)


def test_representation_excludes_tokens():
    endpoint = TokenEndpoint(issued("access-secret-repr"))
    provider = provider_with(endpoint)
    provider()
    for text in (repr(provider), str(provider)):
        assert REFRESH not in text
        assert "access-secret-repr" not in text


def test_exchange_emits_no_secret_logs(caplog):
    endpoint = TokenEndpoint(issued("access-secret-log"))
    with caplog.at_level(logging.DEBUG):
        provider_with(endpoint)()
    assert REFRESH not in caplog.text
    assert "access-secret-log" not in caplog.text


def test_client_sends_the_exchanged_access_token(make_client):
    endpoint = TokenEndpoint(issued("access-for-api"))
    recorder = RecordingTransport(
        responses=[(202, {"requestId": "a"}), (202, {"requestId": "b"})]
    )
    client = make_client(recorder, token_provider=provider_with(endpoint))
    for _ in range(2):
        client.publish(domain="orders", event_name="OrderNotification", data={})
    sent = [request["headers"]["Authorization"] for request in recorder.requests]
    assert sent == ["Bearer access-for-api", "Bearer access-for-api"]
    assert len(endpoint.requests) == 1


def test_client_reports_a_failed_exchange_without_calling_the_api(make_client):
    endpoint = TokenEndpoint((400, {"error": "invalid_grant"}))
    recorder = RecordingTransport(202, {"requestId": "never"})
    client = make_client(recorder, token_provider=provider_with(endpoint))
    with pytest.raises(konductor.AuthenticationError, match="invalid_grant"):
        client.publish(domain="orders", event_name="OrderNotification", data={})
    assert recorder.requests == []


PROXY_VARIABLES = (
    "HTTPS_PROXY",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
)


@pytest.fixture
def proxy_environment(monkeypatch):
    for name in PROXY_VARIABLES:
        monkeypatch.delenv(name, raising=False)
    return monkeypatch


def new_provider():
    return CognitoRefreshTokenProvider(
        oauth_domain=DOMAIN, client_id=CLIENT_ID, refresh_token=REFRESH
    )


def test_token_exchange_uses_the_environment_https_proxy(proxy_environment):
    proxy_environment.setenv("HTTPS_PROXY", "http://proxy.example.invalid:3128")
    provider = new_provider()
    assert isinstance(provider._http, urllib3.ProxyManager)
    assert provider._http.proxy.host == "proxy.example.invalid"
    assert provider._http.proxy.port == 3128


def test_token_exchange_honors_no_proxy(proxy_environment):
    proxy_environment.setenv("HTTPS_PROXY", "http://proxy.example.invalid:3128")
    proxy_environment.setenv("NO_PROXY", "auth.example.invalid")
    provider = new_provider()
    assert not isinstance(provider._http, urllib3.ProxyManager)


def test_token_exchange_connects_directly_without_a_proxy(proxy_environment):
    provider = new_provider()
    assert not isinstance(provider._http, urllib3.ProxyManager)


@pytest.mark.parametrize(
    "domain", ["https://[broken", "https://auth.example.invalid:99999", "https://[::1"]
)
def test_unparseable_oauth_domain_is_a_configuration_error(domain):
    with pytest.raises(konductor.ConfigurationError):
        CognitoRefreshTokenProvider(
            oauth_domain=domain, client_id=CLIENT_ID, refresh_token=REFRESH
        )
