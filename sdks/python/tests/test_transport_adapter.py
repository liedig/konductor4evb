import json

import pytest
import urllib3

from konductor import _generated, _transport

HOST = "https://api.example.invalid/prod"


class RecordingPoolManager:
    def __init__(self):
        self.requests = []
        self.cleared = False

    def request(self, method, url, body=None, headers=None, **kwargs):
        self.requests.append(
            {
                "method": method,
                "url": url,
                "body": body,
                "headers": headers,
                "kwargs": kwargs,
            }
        )
        return urllib3.HTTPResponse(
            body=b'{"requestId": "req-1"}',
            status=202,
            headers={"Content-Type": "application/json"},
            preload_content=True,
        )

    def clear(self):
        self.cleared = True


def spy_signer(label, seen):
    def sign(method, url, body, headers):
        seen.append(
            {"method": method, "url": url, "body": body, "headers": dict(headers)}
        )
        headers["Authorization"] = f"signed-by-{label}"
        return headers

    return sign


@pytest.fixture
def client():
    seen = []
    client = _transport.build_client(HOST, sign=spy_signer("test", seen))
    recorder = RecordingPoolManager()
    client.rest_client.pool_manager.wrapped = recorder
    return client, seen, recorder


def publish_once(client):
    api = _generated.DefaultApi(client)
    request = _generated.PublishRequest(
        domain="orders", event_name="OrderNotification", data=None
    )
    return api.publish_event_with_http_info(publish_request=request)


def test_signer_sees_final_serialized_bytes_and_url(client):
    client, seen, recorder = client
    publish_once(client)
    assert len(seen) == 1
    signed = seen[0]
    assert signed["method"] == "POST"
    assert signed["url"] == f"{HOST}/publish"
    assert json.loads(signed["body"])["data"] is None


def test_signed_headers_reach_the_wire_unchanged(client):
    client, seen, recorder = client
    publish_once(client)
    sent = recorder.requests[0]
    assert sent["url"] == f"{HOST}/publish"
    assert json.loads(sent["body"])["data"] is None
    assert sent["headers"]["Authorization"] == "signed-by-test"


def test_signing_is_injectable():
    first = _transport.build_client(
        HOST,
        sign=lambda method, url, body, headers: {**headers, "Authorization": "one"},
    )
    second = _transport.build_client(
        HOST,
        sign=lambda method, url, body, headers: {**headers, "Authorization": "two"},
    )
    for client, expected in ((first, "one"), (second, "two")):
        recorder = RecordingPoolManager()
        client.rest_client.pool_manager.wrapped = recorder
        api = _generated.DefaultApi(client)
        request = _generated.PublishRequest(domain="d", event_name="e", data={})
        api.publish_event_with_http_info(publish_request=request)
        assert recorder.requests[0]["headers"]["Authorization"] == expected


def test_explicit_timeouts_reach_the_transport(client):
    client, _, recorder = client
    api = _generated.DefaultApi(client)
    request = _generated.PublishRequest(
        domain="orders", event_name="OrderNotification", data={}
    )
    api.publish_event_with_http_info(
        publish_request=request, _request_timeout=(5.0, 30.0)
    )
    timeout = recorder.requests[0]["kwargs"]["timeout"]
    assert isinstance(timeout, urllib3.Timeout)
    assert timeout.connect_timeout == 5.0
    assert timeout.read_timeout == 30.0


def test_retries_and_redirects_are_disabled_at_the_pool():
    client = _transport.build_client(
        HOST, sign=lambda method, url, body, headers: headers
    )
    retries = client.rest_client.pool_manager.wrapped.connection_pool_kw["retries"]
    assert isinstance(retries, urllib3.Retry)
    assert retries.total == 0
    assert retries.redirect == 0
    assert retries.raise_on_redirect is False
    assert retries.raise_on_status is False


def test_close_releases_pooled_connections(client):
    client, _, recorder = client
    _transport.close_client(client)
    assert recorder.cleared is True
