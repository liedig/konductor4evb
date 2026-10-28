import json
from pathlib import Path

import jsonschema
import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[3]
CONTRACT_PATH = REPO_ROOT / "api" / "openapi-oauth.yaml"
FIXTURE_DIR = REPO_ROOT / "sdks" / "conformance" / "subscription-detail"


@pytest.fixture(scope="module")
def contract():
    return yaml.safe_load(CONTRACT_PATH.read_text(encoding="utf-8"))


def resolve_local_refs(node, components):
    if isinstance(node, dict):
        if "$ref" in node:
            ref = node["$ref"]
            prefix = "#/components/schemas/"
            assert ref.startswith(prefix), f"unexpected non-local reference {ref}"
            return resolve_local_refs(components["schemas"][ref[len(prefix):]], components)
        resolved = {
            key: resolve_local_refs(value, components)
            for key, value in node.items()
            if key != "nullable"
        }
        if node.get("nullable") is True:
            return {"anyOf": [resolved, {"type": "null"}]}
        return resolved
    if isinstance(node, list):
        return [resolve_local_refs(item, components) for item in node]
    return node


def detail_schema(contract):
    components = contract["components"]
    return resolve_local_refs(components["schemas"]["SubscriptionDetail"], components)


def fixture_paths():
    return sorted(FIXTURE_DIR.glob("*.json"))


def test_env_stream_state_describes_serialized_fields(contract):
    properties = contract["components"]["schemas"]["EnvStreamState"]["properties"]
    assert "ruleName" in properties, "EnvStream always serializes ruleName"
    assert "errorDetail" in properties, "failed environments carry errorDetail"
    assert "AWAITING_PROMOTION" in properties["status"]["enum"]


def test_summary_describes_top_level_error_detail(contract):
    summary = contract["components"]["schemas"]["SubscriptionSummary"]
    error_detail = summary["properties"].get("errorDetail")
    assert error_detail is not None, "detail handler always emits errorDetail"
    assert error_detail.get("nullable") is True


def test_detail_removed_at_is_nullable(contract):
    detail = contract["components"]["schemas"]["SubscriptionDetail"]
    removed_at = detail["allOf"][1]["properties"]["removedAt"]
    assert removed_at.get("nullable") is True, "handler emits null before removal"


@pytest.mark.parametrize("fixture_path", fixture_paths(), ids=lambda p: p.stem)
def test_observed_detail_response_validates(contract, fixture_path):
    fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
    jsonschema.validate(fixture, detail_schema(contract))
