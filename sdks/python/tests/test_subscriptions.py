import json
import re
from pathlib import Path

import pytest
from conftest import RecordingTransport

import konductor
from konductor import KonductorClient

CONFORMANCE = Path(__file__).resolve().parents[2] / "conformance"
FIXTURES = sorted(
    path
    for directory in ("subscribe", "unsubscribe")
    for path in (CONFORMANCE / directory).glob("*.json")
)


def load_fixture(path):
    return json.loads(path.read_text(encoding="utf-8"))


def body_json(sent):
    body = sent["body"]
    if isinstance(body, bytes):
        body = body.decode("utf-8")
    return json.loads(body)


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def test_every_fixture_operation_is_implemented():
    operations = {load_fixture(path)["operation"] for path in FIXTURES}
    for operation in operations:
        assert callable(getattr(KonductorClient, operation, None)), (
            f"KonductorClient.{operation} is not implemented"
        )


@pytest.mark.parametrize(
    "path", FIXTURES, ids=lambda path: f"{path.parent.name}/{path.stem}"
)
def test_subscription_fixture_wire_contract(make_client, path):
    fixture = load_fixture(path)
    recorder = RecordingTransport(
        fixture["serviceResponse"]["status"], fixture["serviceResponse"]["body"]
    )
    client = make_client(recorder)
    operation = getattr(client, fixture["operation"])
    arguments = {snake(key): value for key, value in fixture["arguments"].items()}

    receipt = operation(**arguments)

    assert len(recorder.requests) == 1
    sent = recorder.requests[0]
    assert sent["method"] == fixture["expectedMethod"]
    assert sent["url"] == f"https://api.example.invalid{fixture['expectedPath']}"
    expected_body = fixture["expectedRequestBody"]
    if expected_body is None:
        assert not sent["body"]
    else:
        assert body_json(sent) == expected_body
    assert receipt.request_id == fixture["expectedReceipt"]["requestId"]
    assert receipt.message == fixture["expectedReceipt"]["message"]


def test_environments_accept_boolean_shorthand(make_client):
    recorder = RecordingTransport(202, {"requestId": "req-bool"})
    client = make_client(recorder)
    client.subscribe(
        consumer_domain="checkout",
        producer_domain="orders",
        schema_name="acme-orders@OrderNotification",
        environments={"prod": False, "dev": True},
    )
    assert body_json(recorder.requests[0])["environments"] == {
        "prod": {"enabled": False},
        "dev": {"enabled": True},
    }


def test_subscribe_receipt_is_acceptance_not_activation(make_client):
    recorder = RecordingTransport(202, {"requestId": "req-accept"})
    client = make_client(recorder)
    receipt = client.subscribe(
        consumer_domain="checkout",
        producer_domain="orders",
        schema_name="acme-orders@OrderNotification",
    )
    assert type(receipt) is konductor.AcceptedCommand


def test_subscribe_service_error_surfaces(make_client):
    recorder = RecordingTransport(
        403, {"code": "AccessDenied", "message": "no binding"}
    )
    client = make_client(recorder)
    with pytest.raises(konductor.ServiceError) as excinfo:
        client.subscribe(
            consumer_domain="checkout",
            producer_domain="orders",
            schema_name="acme-orders@OrderNotification",
        )
    assert excinfo.value.status == 403
    assert len(recorder.requests) == 1


def test_unsubscribe_sends_exactly_one_bodyless_delete(make_client):
    recorder = RecordingTransport(202, {"requestId": "req-del"})
    client = make_client(recorder)
    receipt = client.unsubscribe(subscription_id="checkout.orders.OrderNotification")
    assert receipt.request_id == "req-del"
    assert len(recorder.requests) == 1
    sent = recorder.requests[0]
    assert sent["method"] == "DELETE"
    assert not sent["body"]
