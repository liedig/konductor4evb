"""Public SDK error hierarchy."""

from collections.abc import Iterable


class KonductorError(Exception):
    """Base class for all konductor-sdk errors."""


class ConfigurationError(KonductorError):
    """The client configuration is invalid; no request was sent."""


class AuthenticationError(ConfigurationError):
    """Credentials could not be resolved; no request was sent."""


class ServiceError(KonductorError):
    """The service answered with a non-success status.

    Service messages and field violations can contain caller payload content,
    so they are attributes only and never appear in str/repr.
    """

    def __init__(
        self,
        *,
        status: int,
        code: str | None = None,
        message: str | None = None,
        request_id: str | None = None,
        reason: str | None = None,
        field_violations: Iterable[tuple[str | None, str | None]] = (),
    ) -> None:
        super().__init__(f"Konductor service error (status={status}, code={code!r})")
        self.status = status
        self.code = code
        self.message = message
        self.request_id = request_id
        self.reason = reason
        self.field_violations = tuple(field_violations)


class TransportError(KonductorError):
    """The request failed or its response was lost.

    The command may still have been accepted; callers decide whether to
    resubmit. The SDK never retries implicitly.
    """


class ProtocolError(KonductorError):
    """The response did not match the contract; acceptance state is unknown."""
