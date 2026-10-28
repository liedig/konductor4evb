import json
import logging

import pytest
from conftest import RecordingTransport

import konductor


def publish(client):
    return client.publish(domain="orders", event_name="OrderNotification", data={})


def accepted():
    return RecordingTransport(202, {"requestId": "req-1"})


def test_fixed_token_is_attached_as_bearer(make_client):
    recorder = accepted()
    publish(make_client(recorder, token="tok-fixed-1"))
    assert recorder.requests[0]["headers"]["Authorization"] == "Bearer tok-fixed-1"


def test_provider_is_resolved_for_each_operation(make_client):
    calls = []

    def provider():
        calls.append(1)
        return "tok-callback"

    recorder = RecordingTransport(
        responses=[(202, {"requestId": "a"}), (202, {"requestId": "b"})]
    )
    client = make_client(recorder, token_provider=provider)
    publish(client)
    publish(client)
    assert len(calls) == 2


def test_rotated_tokens_are_used_without_rebuilding(make_client):
    tokens = iter(["tok-one", "tok-two"])
    recorder = RecordingTransport(
        responses=[(202, {"requestId": "a"}), (202, {"requestId": "b"})]
    )
    client = make_client(recorder, token_provider=lambda: next(tokens))
    publish(client)
    publish(client)
    sent = [request["headers"]["Authorization"] for request in recorder.requests]
    assert sent == ["Bearer tok-one", "Bearer tok-two"]


def test_provider_failure_sends_no_request(make_client):
    def broken():
        raise RuntimeError("vault unavailable")

    recorder = accepted()
    with pytest.raises(konductor.AuthenticationError):
        publish(make_client(recorder, token_provider=broken))
    assert recorder.requests == []


@pytest.mark.parametrize("bad", ["", None, 42])
def test_unusable_provider_token_sends_no_request(make_client, bad):
    recorder = accepted()
    with pytest.raises(konductor.AuthenticationError):
        publish(make_client(recorder, token_provider=lambda: bad))
    assert recorder.requests == []


def test_provider_authentication_errors_pass_through_unchanged(make_client):
    original = konductor.AuthenticationError("refresh token was rejected")

    def provider():
        raise original

    recorder = accepted()
    with pytest.raises(konductor.AuthenticationError) as raised:
        publish(make_client(recorder, token_provider=provider))
    assert raised.value is original
    assert recorder.requests == []


def test_rejected_token_is_not_retried_or_replaced(make_client):
    calls = []

    def provider():
        calls.append(1)
        return "tok-expired"

    recorder = RecordingTransport(401, {"message": "Unauthorized"})
    with pytest.raises(konductor.ServiceError) as raised:
        publish(make_client(recorder, token_provider=provider))
    assert raised.value.status == 401
    assert len(recorder.requests) == 1
    assert len(calls) == 1


def test_token_appears_only_in_the_authorization_header(make_client):
    token = "tok-secret-location"
    recorder = accepted()
    publish(make_client(recorder, token=token))
    request = recorder.requests[0]
    assert request["headers"]["Authorization"] == f"Bearer {token}"
    assert token not in request["url"]
    assert token not in request["body"]
    non_auth_headers = {
        k: v for k, v in request["headers"].items() if k.lower() != "authorization"
    }
    assert token not in json.dumps(non_auth_headers)


def test_token_is_absent_from_client_representation(make_client):
    token = "tok-secret-repr"
    client = make_client(accepted(), token=token)
    assert token not in repr(client)
    assert token not in str(client)


def test_sending_emits_no_secret_logs(make_client, caplog):
    token = "tok-secret-logs"
    with caplog.at_level(logging.DEBUG):
        publish(make_client(accepted(), token=token))
    assert token not in caplog.text
