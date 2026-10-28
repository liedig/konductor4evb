"""Validation shared by the client and the Cognito provider configuration."""

import math
from numbers import Real
from typing import Any

from konductor.errors import ConfigurationError


def seconds(name: str, value: Any, *, allow_zero: bool = False) -> float:
    """Return value as float seconds; positive and finite unless allow_zero."""
    bound = "non-negative" if allow_zero else "positive"
    problem = ConfigurationError(f"{name} must be a {bound} finite number of seconds")
    if isinstance(value, bool) or not isinstance(value, Real):
        raise problem
    try:
        result = float(value)
    except OverflowError:
        raise problem from None
    if not math.isfinite(result) or result < 0 or (result == 0 and not allow_zero):
        raise problem
    return result
