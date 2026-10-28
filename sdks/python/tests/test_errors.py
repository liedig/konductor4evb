import traceback

import pytest
import urllib3
from conftest import RecordingTransport

import konductor


def publish_with(make_client, recorder):
    client = make_client(recorder)
    return client.publish(
        domain="orders", event_name="OrderNotification", data={"orderId": 1}
    )


def test_valid_acceptance_receipt(make_client):
    recorder = RecordingTransport(
        202, {"requestId": "req-1", "message": "Request accepted"}
    )
    receipt = publish_with(make_client, recorder)
    assert receipt.request_id == "req-1"
    assert receipt.message == "Request accepted"
    assert len(recorder.requests) == 1


def test_acceptance_message_is_optional(make_client):
    recorder = RecordingTransport(202, {"requestId": "req-2"})
    receipt = publish_with(make_client, recorder)
    assert receipt.request_id == "req-2"
    assert receipt.message is None


@pytest.mark.parametrize("body", [{"message": "no id"}, {"requestId": ""}, {}])
def test_acceptance_without_nonempty_request_id_is_a_protocol_error(make_client, body):
    recorder = RecordingTransport(202, body)
    with pytest.raises(konductor.ProtocolError) as excinfo:
        publish_with(make_client, recorder)
    assert len(recorder.requests) == 1
    text = str(excinfo.value)
    assert "did not confirm" not in text
    assert "was not accepted" not in text.replace("unknown", "")


@pytest.mark.parametrize("status", [400, 401, 403, 404, 422, 429, 500, 503])
def test_service_errors_expose_status_code_message_and_request_id(make_client, status):
    recorder = RecordingTransport(
        status,
        {"code": "ValidationException", "message": "rejected", "requestId": "req-err"},
    )
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    error = excinfo.value
    assert error.status == status
    assert error.code == "ValidationException"
    assert error.message == "rejected"
    assert error.request_id == "req-err"
    assert len(recorder.requests) == 1


def test_field_violations_are_exposed(make_client):
    recorder = RecordingTransport(
        422,
        {
            "code": "ValidationException",
            "message": "payload violates schema",
            "requestId": "req-422",
            "reason": "fieldValidationFailed",
            "fieldList": [{"name": "data.orderId", "message": "expected string"}],
        },
    )
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    error = excinfo.value
    assert error.reason == "fieldValidationFailed"
    assert error.field_violations == (("data.orderId", "expected string"),)


def test_gateway_message_only_error(make_client):
    recorder = RecordingTransport(403, {"message": "Missing Authentication Token"})
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    error = excinfo.value
    assert error.status == 403
    assert error.message == "Missing Authentication Token"
    assert error.code is None


def test_non_json_error_body_is_a_safe_service_error(make_client):
    recorder = RecordingTransport(
        503, raw_body=b"<html>upstream unavailable</html>", content_type="text/html"
    )
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    error = excinfo.value
    assert error.status == 503
    assert error.code is None
    assert error.message is None
    assert "upstream unavailable" not in str(error)
    assert "upstream unavailable" not in repr(error)


def test_redirect_is_reported_not_followed(make_client):
    recorder = RecordingTransport(302, raw_body=b"", content_type="text/plain")
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    assert excinfo.value.status == 302
    assert len(recorder.requests) == 1


def test_lost_response_is_a_transport_error_without_a_delivery_claim(make_client):
    recorder = RecordingTransport(
        error=urllib3.exceptions.ReadTimeoutError(None, "/publish", "timed out")
    )
    with pytest.raises(konductor.TransportError) as excinfo:
        publish_with(make_client, recorder)
    text = str(excinfo.value)
    assert "may still have been accepted" in text
    assert len(recorder.requests) == 1


def test_connection_failure_is_a_transport_error(make_client):
    recorder = RecordingTransport(
        error=urllib3.exceptions.ConnectTimeoutError(
            None, "api.example.invalid", "timed out"
        )
    )
    with pytest.raises(konductor.TransportError):
        publish_with(make_client, recorder)
    assert len(recorder.requests) == 1


def test_exception_representations_exclude_service_payload_content(make_client):
    recorder = RecordingTransport(
        422,
        {
            "code": "ValidationException",
            "message": "field data.orderId=order-secret-123 rejected",
            "fieldList": [
                {"name": "data.orderId", "message": "order-secret-123 invalid"}
            ],
        },
    )
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    error = excinfo.value
    assert "order-secret-123" not in str(error)
    assert "order-secret-123" not in repr(error)
    assert error.message == "field data.orderId=order-secret-123 rejected"
    assert error.field_violations[0][1] == "order-secret-123 invalid"


def test_service_error_traceback_does_not_chain_the_payload_bearing_cause(make_client):
    recorder = RecordingTransport(
        422,
        {
            "code": "ValidationException",
            "message": "field data.orderId=order-secret-123 rejected",
        },
    )
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    error = excinfo.value
    assert error.__cause__ is None
    assert error.__suppress_context__ is True
    rendered = "".join(traceback.format_exception(error))
    assert "order-secret-123" not in rendered


@pytest.mark.parametrize(
    "raw_body",
    [b"{not json", b'{"requestId": 123}'],
    ids=["malformed-json", "wrongly-typed-request-id"],
)
def test_undecodable_acceptance_is_a_protocol_error(make_client, raw_body):
    recorder = RecordingTransport(202, raw_body=raw_body)
    with pytest.raises(konductor.ProtocolError) as excinfo:
        publish_with(make_client, recorder)
    assert "unknown" in str(excinfo.value)
    assert excinfo.value.__cause__ is None
    assert len(recorder.requests) == 1


def test_invalid_caller_input_is_not_reported_as_a_response_failure(make_client):
    recorder = RecordingTransport(202, {"requestId": "req-1"})
    client = make_client(recorder)
    with pytest.raises(ValueError) as excinfo:
        client.unsubscribe(subscription_id=123)
    assert not isinstance(excinfo.value, konductor.KonductorError)
    assert recorder.requests == []


@pytest.mark.parametrize("field_list", [42, "data.orderId", {"name": "x"}])
def test_non_array_field_list_still_yields_a_service_error(make_client, field_list):
    recorder = RecordingTransport(
        500, {"message": "upstream failure", "fieldList": field_list}
    )
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    assert excinfo.value.status == 500
    assert excinfo.value.field_violations == ()


def test_gateway_request_id_header_fills_a_missing_body_identifier(make_client):
    recorder = RecordingTransport(
        401,
        {"message": "Unauthorized"},
        headers={"x-amzn-RequestId": "gw-req-123"},
    )
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    assert excinfo.value.request_id == "gw-req-123"


def test_gateway_request_id_header_is_read_from_non_json_errors(make_client):
    recorder = RecordingTransport(
        502,
        raw_body=b"<html>bad gateway</html>",
        content_type="text/html",
        headers={"X-Amzn-RequestId": "gw-req-502"},
    )
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    assert excinfo.value.request_id == "gw-req-502"


def test_body_request_id_takes_precedence_over_the_header(make_client):
    recorder = RecordingTransport(
        429,
        {"message": "Too Many Requests", "requestId": "body-req"},
        headers={"x-amzn-RequestId": "gw-req-429"},
    )
    with pytest.raises(konductor.ServiceError) as excinfo:
        publish_with(make_client, recorder)
    assert excinfo.value.request_id == "body-req"
