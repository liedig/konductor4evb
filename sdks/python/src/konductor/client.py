"""Public synchronous client for the Konductor application APIs."""

import json
from collections.abc import Callable, Mapping
from types import TracebackType
from typing import Any, Literal, cast
from urllib.parse import urlsplit

import urllib3

from konductor import _config, _generated, _json, _transport
from konductor.auth import TokenProvider, bearer_signer
from konductor.errors import (
    ConfigurationError,
    KonductorError,
    ProtocolError,
    ServiceError,
    TransportError,
)
from konductor.results import (
    AcceptedCommand,
    EnvironmentDetail,
    SubscriptionDetail,
    freeze_environments,
)


def _validate_endpoint(endpoint: str) -> str:
    if not isinstance(endpoint, str) or not endpoint:
        raise ConfigurationError("endpoint must be a non-empty HTTPS URL")
    try:
        parts = urlsplit(endpoint)
        parts.port  # noqa: B018 - raises ValueError for an out-of-range port
    except ValueError:
        raise ConfigurationError("endpoint is not a valid URL") from None
    if parts.scheme != "https":
        raise ConfigurationError("endpoint must use https")
    if parts.username is not None or parts.password is not None:
        raise ConfigurationError("endpoint must not embed credentials")
    if parts.query or parts.fragment:
        raise ConfigurationError("endpoint must not contain a query or fragment")
    if not parts.hostname:
        raise ConfigurationError("endpoint must name a host")
    return f"https://{parts.netloc}{parts.path.rstrip('/')}"


def _acceptance_receipt(accepted: Any) -> AcceptedCommand:
    request_id = getattr(accepted, "request_id", None)
    if not isinstance(request_id, str) or not request_id:
        raise ProtocolError(
            "the service returned HTTP 202 without a nonempty requestId;"
            " acceptance is unknown"
        )
    return AcceptedCommand(
        request_id=request_id, message=getattr(accepted, "message", None)
    )


def _gateway_request_id(headers: Any) -> str | None:
    """API Gateway's own auth and throttling errors carry the request id only
    in this header, not in the body."""
    for name, value in (headers or {}).items():
        if name.lower() == "x-amzn-requestid" and isinstance(value, str) and value:
            return value
    return None


def _map_api_exception(error: Any) -> KonductorError:
    status = error.status
    if status is None or status == 0:
        reason = error.reason or "connection error"
        return TransportError(
            f"the request failed or its response was lost ({reason}); "
            "the command may still have been accepted"
        )
    if 200 <= status < 300:
        return ProtocolError(
            f"the service returned HTTP {status} with a body that does not"
            " match the contract; acceptance is unknown"
        )
    payload = None
    if error.body:
        try:
            parsed = json.loads(error.body)
        except ValueError:
            parsed = None
        if isinstance(parsed, dict):
            payload = parsed
    payload = payload or {}
    field_list = payload.get("fieldList")
    if not isinstance(field_list, list):
        field_list = []
    violations = [
        (item.get("name"), item.get("message"))
        for item in field_list
        if isinstance(item, dict)
    ]
    return ServiceError(
        status=status,
        code=payload.get("code") if isinstance(payload.get("code"), str) else None,
        message=payload.get("message")
        if isinstance(payload.get("message"), str)
        else None,
        request_id=_optional_str(payload, "requestId")
        or _gateway_request_id(error.headers),
        reason=payload.get("reason")
        if isinstance(payload.get("reason"), str)
        else None,
        field_violations=violations,
    )


def _optional_str(payload: dict[str, Any], name: str) -> str | None:
    value = payload.get(name)
    return value if isinstance(value, str) else None


def _subscription_detail(raw: str | bytes) -> SubscriptionDetail:
    try:
        payload = json.loads(raw)
    except (TypeError, ValueError):
        raise ProtocolError(
            "the detail response is not valid JSON; the subscription state is unknown"
        ) from None
    if not isinstance(payload, dict):
        raise ProtocolError(
            "the detail response is not a JSON object;"
            " the subscription state is unknown"
        )
    status = payload.get("status")
    identity = {
        name: payload.get(name)
        for name in (
            "subscriptionId",
            "consumerDomain",
            "producerDomain",
            "source",
            "detailType",
            "schemaName",
        )
    }
    if (
        not isinstance(status, str)
        or not status
        or any(not isinstance(value, str) or not value for value in identity.values())
    ):
        raise ProtocolError(
            "the detail response is missing contract identity or status; "
            "the subscription state is unknown"
        )
    resolved = {name: cast(str, value) for name, value in identity.items()}
    environments: dict[str, EnvironmentDetail] = {}
    raw_environments = payload.get("environments")
    if isinstance(raw_environments, dict):
        for name, state in raw_environments.items():
            if not isinstance(state, dict):
                continue
            pinned = state.get("pinnedVersion")
            enabled = state.get("enabled")
            environments[name] = EnvironmentDetail(
                enabled=enabled if isinstance(enabled, bool) else None,
                status=_optional_str(state, "status") or "",
                pinned_version=pinned
                if isinstance(pinned, int) and not isinstance(pinned, bool)
                else None,
                rule_name=_optional_str(state, "ruleName"),
                rule_arn=_optional_str(state, "ruleArn"),
                error_detail=_optional_str(state, "errorDetail"),
            )
    return SubscriptionDetail(
        subscription_id=resolved["subscriptionId"],
        consumer_domain=resolved["consumerDomain"],
        producer_domain=resolved["producerDomain"],
        source=resolved["source"],
        detail_type=resolved["detailType"],
        schema_name=resolved["schemaName"],
        status=status,
        environments=freeze_environments(environments),
        error_detail=_optional_str(payload, "errorDetail"),
        created_at=_optional_str(payload, "createdAt"),
        updated_at=_optional_str(payload, "updatedAt"),
        removed_at=_optional_str(payload, "removedAt"),
    )


class KonductorClient:
    """Synchronous client for publish, subscribe, unsubscribe, and
    describe_subscription against one explicitly configured endpoint.

    Requests carry a Cognito (OAuth) access token: pass exactly one of a fixed
    token or a zero-argument token_provider called before every request.
    """

    def __init__(
        self,
        *,
        endpoint: str,
        token: str | None = None,
        token_provider: TokenProvider | None = None,
        connect_timeout: float = _transport.DEFAULT_CONNECT_TIMEOUT,
        read_timeout: float = _transport.DEFAULT_READ_TIMEOUT,
        transport_factory: Callable[..., Any] | None = None,
    ) -> None:
        self._endpoint = _validate_endpoint(endpoint)
        self._sign = bearer_signer(token=token, token_provider=token_provider)
        self._connect_timeout = _config.seconds("connect_timeout", connect_timeout)
        self._read_timeout = _config.seconds("read_timeout", read_timeout)
        self._transport_factory = transport_factory or _transport.build_client
        self._client: Any = None
        self._closed = False

    @property
    def endpoint(self) -> str:
        return self._endpoint

    @property
    def connect_timeout(self) -> float:
        return self._connect_timeout

    @property
    def read_timeout(self) -> float:
        return self._read_timeout

    def _transport_client(self) -> Any:
        if self._closed:
            raise ConfigurationError("client is closed")
        if self._client is None:
            self._client = self._transport_factory(host=self._endpoint, sign=self._sign)
        return self._client

    def publish(
        self,
        *,
        domain: str,
        event_name: str,
        data: Any,
        idempotency_key: str | None = None,
    ) -> AcceptedCommand:
        """Submit one event for admission; returns an acceptance receipt.

        data is required: explicit None publishes JSON null. The optional
        idempotency key is forwarded unchanged; it is not cross-request
        deduplication.
        """
        _json.validate_json_value(data)
        api = _generated.DefaultApi(self._transport_client())
        request = _generated.PublishRequest(
            domain=domain,
            event_name=event_name,
            data=data,
            idempotency_key=idempotency_key,
        )
        accepted = self._invoke(
            lambda: api.publish_event_with_http_info(
                publish_request=request,
                _request_timeout=(self._connect_timeout, self._read_timeout),
            )
        ).data
        return _acceptance_receipt(accepted)

    def subscribe(
        self,
        *,
        consumer_domain: str,
        producer_domain: str,
        schema_name: str,
        environments: Mapping[str, Any] | None = None,
    ) -> AcceptedCommand:
        """Submit a subscription command; returns an acceptance receipt.

        Acceptance is not activation: the service authorizes and materializes
        asynchronously. environments maps environment names to desired state
        (a boolean or {"enabled": bool}); unlisted environments keep the
        service default.
        """
        desired = None
        if environments is not None:
            desired = {}
            for name, state in environments.items():
                if isinstance(state, bool):
                    state = {"enabled": state}
                desired[name] = _generated.EnvDesiredState(enabled=state["enabled"])
        api = _generated.DefaultApi(self._transport_client())
        request = _generated.SubscribeRequest(
            consumer_domain=consumer_domain,
            producer_domain=producer_domain,
            schema_name=schema_name,
            environments=desired,
        )
        accepted = self._invoke(
            lambda: api.subscribe_with_http_info(
                subscribe_request=request,
                _request_timeout=(self._connect_timeout, self._read_timeout),
            )
        ).data
        return _acceptance_receipt(accepted)

    def unsubscribe(self, *, subscription_id: str) -> AcceptedCommand:
        """Submit a removal command for one subscription identifier.

        The identifier is the full {consumerDomain}.{producerDomain}.{detailType}
        string; dotted detail types are preserved. Acceptance is not removal.
        """
        api = _generated.DefaultApi(self._transport_client())
        accepted = self._invoke(
            lambda: api.unsubscribe_with_http_info(
                subscription_id=subscription_id,
                _request_timeout=(self._connect_timeout, self._read_timeout),
            )
        ).data
        return _acceptance_receipt(accepted)

    def describe_subscription(self, *, subscription_id: str) -> SubscriptionDetail:
        """Read the current state of one subscription.

        The result is observed resource state, not the outcome of any
        earlier command; the SDK provides no waiters or polling.
        """
        api = _generated.DefaultApi(self._transport_client())
        response = self._invoke(
            lambda: api.describe_subscription_with_http_info(
                subscription_id=subscription_id,
                _request_timeout=(self._connect_timeout, self._read_timeout),
            )
        )
        return _subscription_detail(response.raw_data)

    def _invoke(self, call: Callable[[], Any]) -> Any:
        try:
            return call()
        except _generated.exceptions.ApiException as error:
            raise _map_api_exception(error) from None
        except urllib3.exceptions.HTTPError as error:
            raise TransportError(
                "the request failed or its response was lost"
                f" ({type(error).__name__}); the command may still have been accepted"
            ) from None

    def close(self) -> None:
        if self._client is not None:
            self._client.close()
        self._closed = True

    def __enter__(self) -> "KonductorClient":
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> Literal[False]:
        self.close()
        return False
