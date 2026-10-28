import copy

import pytest
import yaml

from extractor import (
    SUPPORTED_OPERATION_IDS,
    ExtractionError,
    build_sdk_contract,
    dump_projection,
    extract_projection,
    iter_operations,
)

from pathlib import Path

import generate

REPO_ROOT = Path(__file__).resolve().parents[3]
OAUTH_PATH = REPO_ROOT / "api" / "openapi-oauth.yaml"


def load(path):
    return yaml.safe_load(path.read_text(encoding="utf-8"))


@pytest.fixture(scope="module")
def oauth_doc():
    return load(OAUTH_PATH)


def minimal_operation(operation_id, response_schema="CommandAccepted"):
    return {
        "operationId": operation_id,
        "responses": {
            "202": {
                "description": "accepted",
                "content": {
                    "application/json": {
                        "schema": {"$ref": f"#/components/schemas/{response_schema}"}
                    }
                },
            }
        },
    }


def minimal_doc(**overrides):
    doc = {
        "openapi": "3.0.1",
        "info": {"title": "Contract", "version": "1.0.0"},
        "paths": {
            "/publish": {"post": minimal_operation("PublishEvent")},
            "/subscriptions": {"post": minimal_operation("Subscribe")},
            "/subscriptions/{subscriptionId}": {
                "get": minimal_operation("DescribeSubscription", "SubscriptionDetail"),
                "delete": minimal_operation("Unsubscribe"),
            },
        },
        "components": {
            "schemas": {
                "CommandAccepted": {
                    "type": "object",
                    "properties": {"requestId": {"type": "string"}},
                },
                "SubscriptionDetail": {
                    "allOf": [{"$ref": "#/components/schemas/SubscriptionSummary"}]
                },
                "SubscriptionSummary": {"type": "object"},
            }
        },
    }
    for key, value in overrides.items():
        doc[key] = value
    return doc


def operation_ids(projection):
    return sorted(
        op["operationId"] for _, _, op in iter_operations({"paths": projection["paths"]})
    )


def test_extracts_exactly_the_four_supported_operations(oauth_doc):
    projection = extract_projection(oauth_doc)
    assert operation_ids(projection) == sorted(SUPPORTED_OPERATION_IDS)


def test_transitive_local_references_are_included(oauth_doc):
    projection = extract_projection(oauth_doc)
    schemas = projection["components"]["schemas"]
    for name in [
        "CommandAccepted",
        "ErrorResponse",
        "PublishRequest",
        "SubscribeRequest",
        "EnvDesiredState",
        "SubscriptionDetail",
        "SubscriptionSummary",
        "EnvStreamState",
    ]:
        assert name in schemas, f"transitively referenced schema {name} missing"


def test_unreferenced_components_are_excluded(oauth_doc):
    projection = extract_projection(oauth_doc)
    schemas = projection["components"]["schemas"]
    assert "DomainSummary" not in schemas
    assert "ListSubscriptionsResponse" not in schemas


def test_sdk_contract_is_built_from_the_oauth_surface(oauth_doc):
    projection = build_sdk_contract(oauth_doc)
    assert operation_ids(projection) == sorted(SUPPORTED_OPERATION_IDS)


def test_generation_reads_only_the_oauth_contract():
    expected = dump_projection(build_sdk_contract(load(OAUTH_PATH)))
    assert generate.CONTRACT_PATH == OAUTH_PATH
    assert generate.build_projection_text() == expected


def test_platform_routes_are_excluded(oauth_doc):
    projection = extract_projection(oauth_doc)
    assert "/domains" not in projection["paths"]
    assert "/subscriptions/state" not in projection["paths"]
    assert "/schemas" not in projection["paths"]
    assert "ListSubscriptions" not in operation_ids(projection)


def test_extraction_is_deterministic(oauth_doc):
    first = dump_projection(build_sdk_contract(oauth_doc))
    second = dump_projection(build_sdk_contract(load(OAUTH_PATH)))
    assert first == second


def test_authentication_and_deployment_integration_are_stripped():
    doc = minimal_doc()
    for item in doc["paths"].values():
        for op in item.values():
            op["security"] = [{"oauth2": []}]
            op["x-amazon-apigateway-integration"] = {"type": "aws_proxy"}
            op["x-amazon-apigateway-request-validator"] = "validate-body"
    doc["components"]["securitySchemes"] = {"oauth2": {"type": "oauth2"}}
    doc["servers"] = [{"url": "/{stage}"}]
    projection = build_sdk_contract(doc)
    text = dump_projection(projection)
    assert "x-amazon-apigateway" not in text
    assert "security" not in text
    assert "servers" not in projection


def test_real_contract_projection_has_no_deployment_integration(oauth_doc):
    text = dump_projection(build_sdk_contract(oauth_doc))
    assert "x-amazon-apigateway" not in text
    assert "Fn::Sub" not in text


def test_dangling_reference_fails():
    doc = minimal_doc()
    doc["paths"]["/publish"]["post"]["responses"]["202"]["content"]["application/json"][
        "schema"
    ] = {"$ref": "#/components/schemas/DoesNotExist"}
    with pytest.raises(ExtractionError, match="DoesNotExist"):
        extract_projection(doc)


def test_missing_supported_operation_fails():
    doc = minimal_doc()
    del doc["paths"]["/subscriptions/{subscriptionId}"]["get"]
    with pytest.raises(ExtractionError, match="DescribeSubscription"):
        extract_projection(doc)


def test_response_status_enums_become_strings_but_request_constraints_stay(oauth_doc):
    projection = build_sdk_contract(oauth_doc)
    schemas = projection["components"]["schemas"]
    assert "enum" not in schemas["EnvStreamState"]["properties"]["status"]
    assert "enum" not in schemas["SubscriptionSummary"]["properties"]["status"]
    publish = schemas["PublishRequest"]
    assert publish["additionalProperties"] is False
    assert publish["required"] == ["domain", "eventName", "data"]
    assert publish["properties"]["domain"]["minLength"] == 1
    assert publish["properties"]["idempotencyKey"]["maxLength"] == 128


def test_unrelated_platform_route_addition_keeps_four_operations():
    extended = copy.deepcopy(minimal_doc())
    extended["paths"]["/domains"] = {"post": minimal_operation("RegisterDomain")}
    projection = build_sdk_contract(extended)
    assert operation_ids(projection) == sorted(SUPPORTED_OPERATION_IDS)
    assert "/domains" not in projection["paths"]
