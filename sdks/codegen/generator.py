"""Checksum-verified acquisition of the pinned OpenAPI Generator release."""

import hashlib
import json
import os
import tempfile
import urllib.request
from pathlib import Path

PIN_PATH = Path(__file__).resolve().parent / "openapi-generator-pin.json"
DEFAULT_CACHE_DIR = Path.home() / ".cache" / "konductor-codegen"


class GeneratorPinError(Exception):
    """The generator artifact does not match the recorded pin."""


def load_pin():
    return json.loads(PIN_PATH.read_text(encoding="utf-8"))


def verify_checksums(jar_path, pin):
    payload = Path(jar_path).read_bytes()
    sha256 = hashlib.sha256(payload).hexdigest()
    if sha256 != pin["sha256"]:
        raise GeneratorPinError(
            f"sha256 mismatch for {jar_path}: expected {pin['sha256']}, got {sha256}"
        )
    sha1 = hashlib.sha1(payload).hexdigest()
    if sha1 != pin["sha1"]:
        raise GeneratorPinError(
            f"sha1 mismatch for {jar_path}: expected {pin['sha1']}, got {sha1}"
        )


def _download(url, dest):
    with urllib.request.urlopen(url) as response, open(dest, "wb") as out:
        out.write(response.read())


def ensure_generator_jar(pin=None):
    """Return the verified generator jar, downloading it on first use."""
    pin = pin or load_pin()
    jar = DEFAULT_CACHE_DIR / f"openapi-generator-cli-{pin['version']}.jar"
    if not jar.exists():
        DEFAULT_CACHE_DIR.mkdir(parents=True, exist_ok=True)
        # A per-invocation staging file lets concurrent first runs sharing the
        # cache each download and verify independently; replace() is atomic.
        handle, name = tempfile.mkstemp(
            dir=DEFAULT_CACHE_DIR, prefix=f"{jar.name}.", suffix=".download"
        )
        os.close(handle)
        staging = Path(name)
        try:
            _download(pin["url"], staging)
            verify_checksums(staging, pin)
            staging.replace(jar)
        finally:
            staging.unlink(missing_ok=True)
    verify_checksums(jar, pin)
    return jar


if __name__ == "__main__":
    print(ensure_generator_jar())
