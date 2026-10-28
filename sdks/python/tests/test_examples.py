import importlib
import json
import socket
import sys
from pathlib import Path

import pytest
from conftest import RecordingTransport

from konductor import CognitoRefreshTokenProvider

EXAMPLES_DIR = Path(__file__).resolve().parents[1] / "examples"
sys.path.insert(0, str(EXAMPLES_DIR))

EXAMPLES = ["publish", "publish_with_refresh_token", "subscription_lifecycle"]

DETAIL_AWAITING = {
    "subscriptionId": "checkout.orders.OrderNotification",
    "consumerDomain": "checkout",
    "producerDomain": "orders",
    "source": "acme-orders",
    "detailType": "OrderNotification",
    "schemaName": "acme-orders@OrderNotification",
    "status": "ACTIVE",
    "errorDetail": None,
    "environments": {
        "dev": {
            "pinnedVersion": 3,
            "ruleName": "checkout-OrderNotification",
            "ruleArn": (
                "arn:aws:events:ap-southeast-2:111122223333:rule/bus/"
                "checkout-OrderNotification"
            ),
            "enabled": True,
            "status": "ACTIVE",
        },
        "prod": {
            "ruleName": "checkout-OrderNotification",
            "enabled": True,
            "status": "AWAITING_PROMOTION",
        },
    },
    "createdAt": "2026-10-20T01:15:00Z",
    "updatedAt": "2026-10-20T01:15:12Z",
    "removedAt": None,
}


def test_examples_exist():
    for name in EXAMPLES:
        assert (EXAMPLES_DIR / f"{name}.py").is_file(), f"examples/{name}.py is missing"


def test_importing_examples_has_no_network_side_effects(monkeypatch):
    def blocked(*args, **kwargs):
        raise AssertionError("network access during import")

    monkeypatch.setattr(socket, "create_connection", blocked)
    monkeypatch.setattr(socket.socket, "connect", blocked)
    for name in EXAMPLES:
        importlib.import_module(name)


def run_publish_example(module_name, make_client, capsys):
    module = importlib.import_module(module_name)
    recorder = RecordingTransport(
        202, {"requestId": "req-ex", "message": "Request accepted"}
    )
    client = make_client(recorder)
    receipt = module.run(
        client, domain="orders", event_name="OrderNotification", data={"orderId": "o-1"}
    )
    assert receipt.request_id == "req-ex"
    assert len(recorder.requests) == 1
    body = recorder.requests[0]["body"]
    if isinstance(body, bytes):
        body = body.decode("utf-8")
    assert json.loads(body)["domain"] == "orders"


def test_publish_example(make_client, capsys):
    run_publish_example("publish", make_client, capsys)


def test_publish_with_refresh_token_example(make_client, capsys):
    run_publish_example("publish_with_refresh_token", make_client, capsys)


def test_refresh_token_example_builds_a_provider_without_network(monkeypatch):
    module = importlib.import_module("publish_with_refresh_token")
    monkeypatch.setenv("KONDUCTOR_OAUTH_DOMAIN", "https://auth.example.invalid")
    monkeypatch.setenv("KONDUCTOR_CLIENT_ID", "client-example")
    monkeypatch.setenv("KONDUCTOR_REFRESH_TOKEN", "refresh-example")
    provider = module.token_provider_from_environment()
    assert isinstance(provider, CognitoRefreshTokenProvider)


@pytest.mark.parametrize(
    ("step", "response", "method"),
    [
        ("subscribe", (202, {"requestId": "req-sub"}), "POST"),
        ("describe", (200, DETAIL_AWAITING), "GET"),
        ("publish", (202, {"requestId": "req-pub"}), "POST"),
        ("unsubscribe", (202, {"requestId": "req-unsub"}), "DELETE"),
    ],
)
def test_subscription_lifecycle_steps_run_separately(
    make_client, capsys, step, response, method
):
    module = importlib.import_module("subscription_lifecycle")
    recorder = RecordingTransport(responses=[response])
    client = make_client(recorder, token="tok-example")

    module.STEPS[step](client)

    assert [request["method"] for request in recorder.requests] == [method]
    output = capsys.readouterr().out.lower()
    assert "ready" not in output
    assert "activated" not in output


def test_describe_step_reports_observed_environment_state(make_client, capsys):
    module = importlib.import_module("subscription_lifecycle")
    recorder = RecordingTransport(responses=[(200, DETAIL_AWAITING)])

    module.STEPS["describe"](make_client(recorder, token="tok-example"))

    assert "awaiting_promotion" in capsys.readouterr().out.lower()


def test_describe_step_handles_a_subscription_not_recorded_yet(make_client, capsys):
    module = importlib.import_module("subscription_lifecycle")
    recorder = RecordingTransport(
        responses=[(404, {"message": "Subscription not found", "requestId": "r"})]
    )

    module.STEPS["describe"](make_client(recorder, token="tok-example"))

    output = capsys.readouterr().out.lower()
    assert "no subscription record yet" in output


def test_subscription_lifecycle_requires_an_explicit_step():
    module = importlib.import_module("subscription_lifecycle")
    with pytest.raises(SystemExit):
        module.main([])


def test_examples_contain_no_credentials_or_provisioning():
    for name in EXAMPLES:
        source = (EXAMPLES_DIR / f"{name}.py").read_text(encoding="utf-8")
        lowered = source.lower()
        for marker in (
            "akia",
            "secret",
            "password=",
            "register_domain",
            "createschema",
            "put_rule",
        ):
            assert marker not in lowered, f"{name}.py contains {marker!r}"
