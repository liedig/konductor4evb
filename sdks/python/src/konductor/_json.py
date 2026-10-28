"""Strict JSON-compatibility validation for caller-supplied payloads.

The wire format is JSON: reject anything the encoder would silently stringify
or coerce rather than letting invalid or lossy payloads reach the transport.
Object keys are caller payload, so error paths render them as ``<key>`` and
keep only array positions.
"""

import math
from typing import Any


def validate_json_value(value: Any, path: str = "data") -> None:
    if value is None or isinstance(value, (str, bool, int)):
        return
    if isinstance(value, float):
        if not math.isfinite(value):
            raise ValueError(
                f"{path} contains a non-finite number, which is not valid JSON"
            )
        return
    if isinstance(value, list):
        for index, item in enumerate(value):
            validate_json_value(item, f"{path}[{index}]")
        return
    if isinstance(value, dict):
        for key, item in value.items():
            if not isinstance(key, str):
                raise TypeError(
                    f"{path} contains a {type(key).__name__} object key;"
                    " JSON object keys must be strings"
                )
            validate_json_value(item, f"{path}.<key>")
        return
    raise TypeError(
        f"{path} contains {type(value).__name__}, which is not a JSON value"
    )
