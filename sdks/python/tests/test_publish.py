import copy
import json
import math
from datetime import UTC, datetime
from pathlib import Path

import pytest
import urllib3

from konductor import KonductorClient

FIXTURE_DIR = Path(__file__).resolve().parents[2] / "conformance" / "publish"
ENDPOINT = "https://api.example.invalid/prod"


class RecordingTransport:
    """Stands in for the generated stack: applies the auth signer, records the
    request, and returns the fixture's service response."""

    def __init__(self, fixture):
        self.fixture = fixture
        self.requests = []
        self.closed = False

    def request(self, method, url, body=None, headers=None, **kwargs):
        self.requests.append(
            {"method": method, "url": url, "body": body, "headers": headers}
        )
        response = self.fixture["serviceResponse"]
        return urllib3.HTTPResponse(
            body=json.dumps(response["body"]).encode("utf-8"),
            status=response["status"],
            headers={"Content-Type": "application/json"},
            preload_content=True,
        )

    def clear(self):
        self.closed = True


def client_with_recorder(fixture):
    holder = {}

    def factory(host, sign):
        from konductor import _transport

        client = _transport.build_client(host, sign=sign)
        recorder = RecordingTransport(fixture)
        client.rest_client.pool_manager.wrapped = recorder
        holder["recorder"] = recorder
        return client

    client = KonductorClient(
        endpoint=ENDPOINT, token="tok-fixture", transport_factory=factory
    )
    return client, holder


def fixture_paths():
    return sorted(FIXTURE_DIR.glob("*.json"))


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


@pytest.mark.parametrize("fixture_path", fixture_paths(), ids=lambda p: p.stem)
def test_publish_sends_the_exact_fixture_body(fixture_path):
    fixture = load(fixture_path)
    client, holder = client_with_recorder(fixture)
    argument_names = {"eventName": "event_name", "idempotencyKey": "idempotency_key"}
    arguments = {
        argument_names.get(key, key): value
        for key, value in fixture["arguments"].items()
    }
    receipt = client.publish(**arguments)
    assert len(holder["recorder"].requests) == 1
    sent = holder["recorder"].requests[0]
    assert sent["method"] == "POST"
    assert sent["url"] == f"{ENDPOINT}/publish"
    assert json.loads(sent["body"]) == fixture["expectedRequestBody"]
    assert receipt.request_id == fixture["expectedReceipt"]["requestId"]
    assert receipt.message == fixture["expectedReceipt"].get("message")


def test_publish_preserves_value_kinds_on_the_wire():
    fixture = load(FIXTURE_DIR / "all-json-kinds.json")
    client, holder = client_with_recorder(fixture)
    client.publish(
        domain="orders",
        event_name="OrderNotification",
        data=fixture["arguments"]["data"],
    )
    sent = json.loads(holder["recorder"].requests[0]["body"])["data"]
    assert sent["truthy"] is True
    assert sent["falsy"] is False
    assert isinstance(sent["integer"], int)
    assert sent["integer"] == 9007199254740993
    assert sent["nothing"] is None and "nothing" in sent


def test_publish_does_not_modify_caller_input():
    fixture = load(FIXTURE_DIR / "all-json-kinds.json")
    client, _ = client_with_recorder(fixture)
    data = fixture["arguments"]["data"]
    snapshot = copy.deepcopy(data)
    client.publish(domain="orders", event_name="OrderNotification", data=data)
    assert data == snapshot


def test_missing_data_is_rejected_locally():
    fixture = load(FIXTURE_DIR / "explicit-null-data.json")
    client, holder = client_with_recorder(fixture)
    with pytest.raises(TypeError):
        client.publish(domain="orders", event_name="OrderNotification")
    assert holder.get("recorder") is None or holder["recorder"].requests == []


@pytest.mark.parametrize(
    "bad_data",
    [
        {"nan": math.nan},
        {"inf": math.inf},
        {"set": {1, 2}},
        {"object": object()},
        {"bytes": b"raw"},
        {"when": datetime(2026, 10, 26, tzinfo=UTC)},
        {1: "non-string key"},
    ],
    ids=["nan", "inf", "set", "object", "bytes", "datetime", "int-key"],
)
def test_invalid_json_never_reaches_the_transport(bad_data):
    fixture = load(FIXTURE_DIR / "all-json-kinds.json")
    client, holder = client_with_recorder(fixture)
    with pytest.raises((TypeError, ValueError)):
        client.publish(domain="orders", event_name="OrderNotification", data=bad_data)
    assert holder.get("recorder") is None or holder["recorder"].requests == []
