"""Every SDK failure path keeps secrets out of rendered errors.

Each case drives one failure with a marker secret placed where a real
secret could appear (token, refresh token, provider exception, service or
token-endpoint body, transport error text), then renders the resulting
error the way logging.exception or an uncaught traceback would.
"""

import json
import traceback

import pytest
import urllib3
from conftest import RecordingTransport

import konductor
from konductor import CognitoRefreshTokenProvider, KonductorClient

MARKER = "leak-marker-7f3a"
ENDPOINT = "https://api.example.invalid/prod"


def rendered(error):
    return "".join(traceback.format_exception(error)) + str(error) + repr(error)


def publish(client):
    return client.publish(domain="orders", event_name="OrderNotification", data={})


class TokenEndpoint:
    def __init__(self, response):
        self.response = response

    def request(self, method, url, body=None, headers=None, **kwargs):
        if isinstance(self.response, Exception):
            raise self.response
        status, raw = self.response
        return urllib3.HTTPResponse(
            body=raw,
            status=status,
            headers={"Content-Type": "application/json"},
            preload_content=True,
        )


def cognito_provider(response):
    provider = CognitoRefreshTokenProvider(
        oauth_domain="https://auth.example.invalid",
        client_id="client-abc",
        refresh_token=f"refresh-{MARKER}",
    )
    provider._http = TokenEndpoint(response)
    return provider


def fixed_token_with_line_break(make_client):
    return KonductorClient(endpoint=ENDPOINT, token=f"{MARKER}\r\nX-Injected: 1")


def provider_token_with_line_break(make_client):
    recorder = RecordingTransport(202, {"requestId": "req-1"})
    publish(make_client(recorder, token_provider=lambda: f"{MARKER}\n"))


def provider_raises_with_token_text(make_client):
    def provider():
        raise RuntimeError(f"vault rejected access token {MARKER}")

    publish(make_client(RecordingTransport(202, {}), token_provider=provider))


def cognito_rejects(make_client):
    body = json.dumps({"error": "invalid_grant", "error_description": MARKER})
    provider = cognito_provider((400, body.encode()))
    publish(make_client(RecordingTransport(202, {}), token_provider=provider))


def cognito_unreachable(make_client):
    provider = cognito_provider(urllib3.exceptions.ProtocolError(f"lost {MARKER}"))
    publish(make_client(RecordingTransport(202, {}), token_provider=provider))


def cognito_malformed(make_client):
    provider = cognito_provider((200, json.dumps({"echo": MARKER}).encode()))
    publish(make_client(RecordingTransport(202, {}), token_provider=provider))


def service_error(status):
    def run(make_client):
        recorder = RecordingTransport(
            status,
            {
                "code": "ValidationException",
                "message": f"data.orderId={MARKER} rejected",
                "fieldList": [{"name": "data.orderId", "message": MARKER}],
            },
        )
        publish(make_client(recorder))

    return run


def undecodable_acceptance(make_client):
    raw = f'{{"requestId": 1, "x": "{MARKER}"'.encode()
    recorder = RecordingTransport(202, raw_body=raw)
    publish(make_client(recorder))


def undecodable_detail(make_client):
    raw = json.dumps({"status": "ACTIVE", "createdAt": MARKER}).encode()
    client = make_client(RecordingTransport(200, raw_body=raw))
    client.describe_subscription(subscription_id="checkout.orders.OrderNotification")


def transport_error(make_client):
    error = urllib3.exceptions.ProxyError(
        f"Unable to connect to proxy http://user:{MARKER}@proxy.invalid", None
    )
    publish(make_client(RecordingTransport(error=error)))


FAILURE_PATHS = {
    "fixed-token-line-break": fixed_token_with_line_break,
    "provider-token-line-break": provider_token_with_line_break,
    "provider-raises-with-token-text": provider_raises_with_token_text,
    "cognito-rejects": cognito_rejects,
    "cognito-unreachable": cognito_unreachable,
    "cognito-malformed": cognito_malformed,
    "service-error-422": service_error(422),
    "service-error-500": service_error(500),
    "undecodable-acceptance": undecodable_acceptance,
    "undecodable-detail": undecodable_detail,
    "transport-error": transport_error,
}


@pytest.mark.parametrize("path", FAILURE_PATHS.values(), ids=FAILURE_PATHS.keys())
def test_failure_never_renders_the_secret(make_client, path):
    with pytest.raises(konductor.KonductorError) as excinfo:
        path(make_client)
    assert MARKER not in rendered(excinfo.value)


@pytest.mark.parametrize("token", ["tok\r", "tok\n", "to\r\nk"])
def test_line_break_in_a_fixed_token_is_rejected_before_any_request(token):
    with pytest.raises(konductor.AuthenticationError):
        KonductorClient(endpoint=ENDPOINT, token=token)


def test_line_break_in_a_provider_token_sends_no_request(make_client):
    recorder = RecordingTransport(202, {"requestId": "req-1"})
    with pytest.raises(konductor.AuthenticationError):
        publish(make_client(recorder, token_provider=lambda: "tok\n"))
    assert recorder.requests == []


def test_service_error_attributes_stay_readable(make_client):
    with pytest.raises(konductor.ServiceError) as excinfo:
        service_error(422)(make_client)
    assert MARKER in excinfo.value.message
    assert excinfo.value.field_violations == (("data.orderId", MARKER),)


@pytest.mark.parametrize(
    "data",
    [
        {f"customer-{MARKER}@example.com": float("nan")},
        {"orders": [{f"{MARKER}": object()}]},
        {(MARKER,): "tuple key"},
    ],
    ids=["non-finite-value", "unsupported-value", "non-string-key"],
)
def test_invalid_payload_errors_never_render_object_keys(make_client, data):
    recorder = RecordingTransport(202, {"requestId": "req-1"})
    client = make_client(recorder)
    with pytest.raises((TypeError, ValueError)) as excinfo:
        client.publish(domain="orders", event_name="OrderNotification", data=data)
    assert MARKER not in rendered(excinfo.value)
    assert recorder.requests == []


def test_invalid_payload_errors_still_locate_array_positions(make_client):
    client = make_client(RecordingTransport(202, {"requestId": "req-1"}))
    with pytest.raises(ValueError, match=r"data\.<key>\[2\]"):
        client.publish(
            domain="orders",
            event_name="OrderNotification",
            data={"amounts": [1.0, 2.0, float("inf")]},
        )
