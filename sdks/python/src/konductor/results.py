"""Public result types returned by the SDK's operations."""

import dataclasses
from collections.abc import Mapping
from types import MappingProxyType


@dataclasses.dataclass(frozen=True)
class AcceptedCommand:
    """The service accepted the command for processing.

    Acceptance is not completion: a queued command can still fail during
    processing, and publish acceptance says nothing about delivery.
    """

    request_id: str
    message: str | None = None


@dataclasses.dataclass(frozen=True)
class EnvironmentDetail:
    """One environment's observed subscription state.

    Statuses are reported verbatim; values outside the documented set are
    preserved as plain strings. Optional fields are None when the service
    omitted them or returned null.
    """

    enabled: bool | None
    status: str
    pinned_version: int | None = None
    rule_name: str | None = None
    rule_arn: str | None = None
    error_detail: str | None = None


@dataclasses.dataclass(frozen=True)
class SubscriptionDetail:
    """Current subscription resource state.

    This is an observed snapshot, not the outcome of any previously
    submitted command; there is no correlation between an AcceptedCommand
    receipt and a later detail read.
    """

    subscription_id: str
    consumer_domain: str
    producer_domain: str
    source: str
    detail_type: str
    schema_name: str
    status: str
    environments: Mapping[str, EnvironmentDetail]
    error_detail: str | None = None
    created_at: str | None = None
    updated_at: str | None = None
    removed_at: str | None = None


def freeze_environments(
    environments: Mapping[str, EnvironmentDetail],
) -> Mapping[str, EnvironmentDetail]:
    return MappingProxyType(dict(environments))
