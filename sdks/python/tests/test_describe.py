import json
from pathlib import Path

import pytest
from conftest import RecordingTransport

import konductor

FIXTURES = sorted(
    (Path(__file__).resolve().parents[2] / "conformance" / "subscription-detail").glob(
        "*.json"
    )
)
SUBSCRIPTION_ID = "checkout.orders.OrderNotification"


def load_fixture(path):
    return json.loads(path.read_text(encoding="utf-8"))


def to_plain(detail):
    return {
        "subscriptionId": detail.subscription_id,
        "consumerDomain": detail.consumer_domain,
        "producerDomain": detail.producer_domain,
        "source": detail.source,
        "detailType": detail.detail_type,
        "schemaName": detail.schema_name,
        "status": detail.status,
        "errorDetail": detail.error_detail,
        "environments": {
            name: {
                "pinnedVersion": env.pinned_version,
                "ruleName": env.rule_name,
                "ruleArn": env.rule_arn,
                "enabled": env.enabled,
                "status": env.status,
                "errorDetail": env.error_detail,
            }
            for name, env in detail.environments.items()
        },
        "createdAt": detail.created_at,
        "updatedAt": detail.updated_at,
        "removedAt": detail.removed_at,
    }


def expected_from(fixture):
    environments = {
        name: {
            "pinnedVersion": env.get("pinnedVersion"),
            "ruleName": env.get("ruleName"),
            "ruleArn": env.get("ruleArn"),
            "enabled": env["enabled"],
            "status": env["status"],
            "errorDetail": env.get("errorDetail"),
        }
        for name, env in fixture["environments"].items()
    }
    return {
        **{
            key: fixture[key]
            for key in (
                "subscriptionId",
                "consumerDomain",
                "producerDomain",
                "source",
                "detailType",
                "schemaName",
                "status",
                "errorDetail",
                "createdAt",
                "updatedAt",
                "removedAt",
            )
        },
        "environments": environments,
    }


@pytest.mark.parametrize("path", FIXTURES, ids=lambda path: path.stem)
def test_detail_fixtures_preserve_every_field(make_client, path):
    fixture = load_fixture(path)
    recorder = RecordingTransport(200, fixture)
    client = make_client(recorder)

    detail = client.describe_subscription(subscription_id=fixture["subscriptionId"])

    assert len(recorder.requests) == 1
    sent = recorder.requests[0]
    assert sent["method"] == "GET"
    assert sent["url"] == (
        f"https://api.example.invalid/prod/subscriptions/{fixture['subscriptionId']}"
    )
    assert not sent["body"]
    assert to_plain(detail) == expected_from(fixture)


def test_partial_activation_states_stay_distinct(make_client):
    fixture = load_fixture(FIXTURES[0].parent / "partial-activation.json")
    recorder = RecordingTransport(200, fixture)
    client = make_client(recorder)

    detail = client.describe_subscription(subscription_id=SUBSCRIPTION_ID)

    assert detail.environments["dev"].status == "ACTIVE"
    assert detail.environments["dev"].pinned_version == 3
    assert detail.environments["prod"].status == "AWAITING_PROMOTION"
    assert detail.environments["prod"].pinned_version is None
    assert detail.environments["prod"].rule_arn is None


def test_unknown_status_strings_are_preserved(make_client):
    fixture = load_fixture(FIXTURES[0].parent / "partial-activation.json")
    fixture["status"] = "MIGRATING"
    fixture["environments"]["dev"]["status"] = "SYNCING"
    recorder = RecordingTransport(200, fixture)
    client = make_client(recorder)

    detail = client.describe_subscription(subscription_id=SUBSCRIPTION_ID)

    assert detail.status == "MIGRATING"
    assert detail.environments["dev"].status == "SYNCING"


def test_additional_response_fields_are_tolerated(make_client):
    fixture = load_fixture(FIXTURES[0].parent / "partial-activation.json")
    fixture["futureField"] = {"nested": True}
    fixture["environments"]["dev"]["futureFlag"] = "x"
    recorder = RecordingTransport(200, fixture)
    client = make_client(recorder)

    detail = client.describe_subscription(subscription_id=SUBSCRIPTION_ID)

    assert detail.status == "ACTIVE"


def test_describe_errors_are_service_errors_not_state(make_client):
    for status in (403, 404):
        recorder = RecordingTransport(
            status, {"code": "X", "message": "m", "requestId": "r"}
        )
        client = make_client(recorder)
        with pytest.raises(konductor.ServiceError) as excinfo:
            client.describe_subscription(subscription_id=SUBSCRIPTION_ID)
        assert excinfo.value.status == status
        assert len(recorder.requests) == 1


def test_read_after_command_returns_observed_state_not_command_outcome(make_client):
    fixture = load_fixture(FIXTURES[0].parent / "partial-activation.json")
    recorder = RecordingTransport(
        responses=[(202, {"requestId": "req-cmd"}), (200, fixture)]
    )
    client = make_client(recorder)

    client.unsubscribe(subscription_id=SUBSCRIPTION_ID)
    detail = client.describe_subscription(subscription_id=SUBSCRIPTION_ID)

    assert detail.status == "ACTIVE"
    assert len(recorder.requests) == 2


@pytest.mark.parametrize(
    "raw_body",
    [b"{not json", b'{"status": "ACTIVE", "createdAt": "not-a-timestamp"}'],
    ids=["malformed-json", "contract-type-mismatch"],
)
def test_undecodable_detail_is_a_protocol_error(make_client, raw_body):
    recorder = RecordingTransport(200, raw_body=raw_body)
    client = make_client(recorder)
    with pytest.raises(konductor.ProtocolError):
        client.describe_subscription(subscription_id=SUBSCRIPTION_ID)
    assert len(recorder.requests) == 1
