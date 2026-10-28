import io
import json

import pytest
import urllib3

from konductor import KonductorClient

ENDPOINT = "https://api.example.invalid/prod"


class RecordingTransport:
    """Stands in for the network: records the signed request and returns a
    canned response."""

    def __init__(
        self,
        status=202,
        body=None,
        raw_body=None,
        content_type="application/json",
        error=None,
        responses=None,
        headers=None,
    ):
        self.responses = list(responses) if responses is not None else None
        self.extra_headers = dict(headers or {})
        self.status = status
        self.raw_body = (
            raw_body if raw_body is not None else json.dumps(body or {}).encode("utf-8")
        )
        self.content_type = content_type
        self.error = error
        self.requests = []
        self.closed = False

    def request(self, method, url, body=None, headers=None, **kwargs):
        self.requests.append(
            {"method": method, "url": url, "body": body, "headers": headers}
        )
        if self.error is not None:
            raise self.error
        if self.responses is not None:
            status, body = self.responses.pop(0)
            return urllib3.HTTPResponse(
                body=io.BytesIO(json.dumps(body).encode("utf-8")),
                status=status,
                headers={"Content-Type": "application/json"},
                preload_content=False,
            )
        return urllib3.HTTPResponse(
            body=io.BytesIO(self.raw_body),
            status=self.status,
            headers={"Content-Type": self.content_type, **self.extra_headers},
            preload_content=False,
        )

    def clear(self):
        self.closed = True


@pytest.fixture
def make_client():
    def make(recorder, token=None, token_provider=None):
        def factory(host, sign):
            from konductor import _transport

            client = _transport.build_client(host, sign=sign)
            client.rest_client.pool_manager.wrapped = recorder
            return client

        if token is None and token_provider is None:
            token = "tok-test"
        return KonductorClient(
            endpoint=ENDPOINT,
            token=token,
            token_provider=token_provider,
            transport_factory=factory,
        )

    return make
