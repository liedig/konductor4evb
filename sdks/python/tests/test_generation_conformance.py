import json
import re
import tomllib
from pathlib import Path

import pytest
import urllib3

from konductor import _generated

FIXTURE_DIR = (
    Path(__file__).resolve().parents[2] / "conformance" / "subscription-detail"
)
HOST = "https://api.example.invalid/prod"


class RecordingPoolManager:
    def __init__(self):
        self.requests = []
        self.next_status = 202
        self.next_body = b'{"requestId": "req-1", "message": "Request accepted"}'

    def respond_with(self, status, payload):
        self.next_status = status
        self.next_body = json.dumps(payload).encode("utf-8")

    def request(self, method, url, body=None, headers=None, **kwargs):
        self.requests.append(
            {"method": method, "url": url, "body": body, "headers": headers}
        )
        return urllib3.HTTPResponse(
            body=self.next_body,
            status=self.next_status,
            headers={"Content-Type": "application/json"},
            preload_content=True,
        )

    def clear(self):
        pass


@pytest.fixture
def api():
    configuration = _generated.Configuration(host=HOST)
    client = _generated.ApiClient(configuration)
    recorder = RecordingPoolManager()
    client.rest_client.pool_manager = recorder
    return _generated.DefaultApi(client), recorder


def published_body(recorder):
    assert len(recorder.requests) == 1
    return json.loads(recorder.requests[0]["body"])


def test_required_null_data_is_serialized(api):
    api_client, recorder = api
    request = _generated.PublishRequest(
        domain="orders", event_name="OrderNotification", data=None
    )
    api_client.publish_event_with_http_info(publish_request=request)
    body = published_body(recorder)
    assert "data" in body
    assert body["data"] is None


def test_every_json_value_kind_round_trips(api):
    api_client, recorder = api
    data = {
        "object": {"nested": {"deep": [1, {"x": "y"}]}},
        "array": [1, "two", False, None, [3]],
        "string": "plain",
        "integer": 42,
        "float": 99.5,
        "truthy": True,
        "falsy": False,
        "nothing": None,
    }
    request = _generated.PublishRequest(
        domain="orders", event_name="OrderNotification", data=data
    )
    api_client.publish_event_with_http_info(publish_request=request)
    assert published_body(recorder)["data"] == data


def test_nested_unicode_is_preserved(api):
    api_client, recorder = api
    data = {"greeting": "héllo ☃ こんにちは", "emoji": "🚀"}
    request = _generated.PublishRequest(
        domain="orders", event_name="OrderNotification", data=data
    )
    api_client.publish_event_with_http_info(publish_request=request)
    assert published_body(recorder)["data"] == data


def test_booleans_are_not_coerced_to_integers(api):
    api_client, recorder = api
    data = {"flag": True, "off": False}
    request = _generated.PublishRequest(
        domain="orders", event_name="OrderNotification", data=data
    )
    api_client.publish_event_with_http_info(publish_request=request)
    sent = published_body(recorder)["data"]
    assert sent["flag"] is True
    assert sent["off"] is False


def test_integers_above_float_precision_stay_exact(api):
    api_client, recorder = api
    big = 2**53 + 1
    request = _generated.PublishRequest(
        domain="orders", event_name="OrderNotification", data={"n": big}
    )
    api_client.publish_event_with_http_info(publish_request=request)
    sent = published_body(recorder)["data"]["n"]
    assert isinstance(sent, int)
    assert sent == big


def test_optional_idempotency_key_omission_and_presence(api):
    api_client, recorder = api
    request = _generated.PublishRequest(
        domain="orders", event_name="OrderNotification", data={}
    )
    api_client.publish_event_with_http_info(publish_request=request)
    assert "idempotencyKey" not in published_body(recorder)

    recorder.requests.clear()
    keyed = _generated.PublishRequest(
        domain="orders",
        event_name="OrderNotification",
        data={},
        idempotency_key="caller-key-7",
    )
    api_client.publish_event_with_http_info(publish_request=keyed)
    assert published_body(recorder)["idempotencyKey"] == "caller-key-7"


def load_detail(api_client, fixture_name):
    fixture = json.loads((FIXTURE_DIR / fixture_name).read_text(encoding="utf-8"))
    api_client.api_client.rest_client.pool_manager.respond_with(200, fixture)
    return api_client.describe_subscription_with_http_info(
        subscription_id="checkout.orders.OrderNotification"
    ).data


def test_composed_detail_decodes_partial_activation(api):
    api_client, _ = api
    detail = load_detail(api_client, "partial-activation.json")
    assert detail.status == "ACTIVE"
    assert detail.environments["dev"].status == "ACTIVE"
    assert detail.environments["dev"].pinned_version == 3
    prod = detail.environments["prod"]
    assert prod.status == "AWAITING_PROMOTION"
    assert prod.pinned_version is None
    assert prod.rule_arn is None
    assert prod.rule_name == "checkout-OrderNotification"


def test_nullable_top_level_fields_decode(api):
    api_client, _ = api
    detail = load_detail(api_client, "partial-activation.json")
    assert detail.error_detail is None
    assert detail.removed_at is None


def test_failure_details_decode(api):
    api_client, _ = api
    detail = load_detail(api_client, "failed.json")
    assert detail.status == "FAILED"
    assert "not found in any environment" in detail.error_detail
    assert detail.environments["staging"].error_detail == "PutRule throttled"


def test_removed_timestamp_decodes(api):
    api_client, _ = api
    detail = load_detail(api_client, "removed.json")
    assert detail.status == "REMOVED"
    assert detail.removed_at is not None


def test_unknown_status_is_preserved_not_rejected(api):
    api_client, recorder = api
    fixture = json.loads(
        (FIXTURE_DIR / "partial-activation.json").read_text(encoding="utf-8")
    )
    fixture["status"] = "MIGRATING"
    fixture["environments"]["dev"]["status"] = "MIGRATING"
    recorder.respond_with(200, fixture)
    detail = api_client.describe_subscription_with_http_info(
        subscription_id="checkout.orders.OrderNotification"
    ).data
    assert detail.status == "MIGRATING"
    assert detail.environments["dev"].status == "MIGRATING"


def test_dependency_floor_supports_the_generated_model_configuration():
    generated_models = Path(_generated.__file__).parent / "models"
    uses_validate_by_name = any(
        "validate_by_name=True" in path.read_text(encoding="utf-8")
        for path in generated_models.glob("*.py")
    )
    pyproject = tomllib.loads(
        (Path(__file__).resolve().parents[1] / "pyproject.toml").read_text()
    )
    pydantic = next(
        dep
        for dep in pyproject["project"]["dependencies"]
        if dep.startswith("pydantic")
    )
    floor = re.search(r">=(\d+)\.(\d+)", pydantic)
    assert floor is not None
    assert uses_validate_by_name
    assert (int(floor.group(1)), int(floor.group(2))) >= (2, 11), (
        "validate_by_name/validate_by_alias need pydantic 2.11 or later"
    )
