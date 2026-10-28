import hashlib
import json
from pathlib import Path

import pytest

import generator

PIN_PATH = Path(generator.__file__).parent / "openapi-generator-pin.json"


def test_pin_records_an_exact_release_with_checksums():
    pin = json.loads(PIN_PATH.read_text(encoding="utf-8"))
    assert pin["version"] == "7.26.0"
    assert pin["url"].endswith(f"openapi-generator-cli-{pin['version']}.jar")
    assert len(pin["sha256"]) == 64
    assert len(pin["sha1"]) == 40
    assert pin["generator"] == "python"
    assert pin["library"] == "urllib3"


def test_verify_accepts_the_pinned_jar(tmp_path):
    pin = generator.load_pin()
    jar = tmp_path / "generator.jar"
    jar.write_bytes(b"fake jar bytes")
    pin["sha256"] = hashlib.sha256(b"fake jar bytes").hexdigest()
    pin["sha1"] = hashlib.sha1(b"fake jar bytes").hexdigest()
    generator.verify_checksums(jar, pin)


def test_verify_rejects_a_corrupted_download(tmp_path):
    pin = generator.load_pin()
    jar = tmp_path / "generator.jar"
    jar.write_bytes(b"tampered")
    with pytest.raises(generator.GeneratorPinError, match="sha256"):
        generator.verify_checksums(jar, pin)


def test_generator_url_downloads_to_cache_and_verifies(tmp_path, monkeypatch):
    payload = b"downloaded jar"
    pin = {
        "version": "0.0.0",
        "url": "https://example.invalid/generator.jar",
        "sha256": hashlib.sha256(payload).hexdigest(),
        "sha1": hashlib.sha1(payload).hexdigest(),
        "generator": "python",
        "library": "urllib3",
    }
    monkeypatch.setattr(generator, "DEFAULT_CACHE_DIR", tmp_path)
    monkeypatch.setattr(generator, "_download", lambda url, dest: dest.write_bytes(payload))
    jar = generator.ensure_generator_jar(pin)
    assert jar.read_bytes() == payload
    again = generator.ensure_generator_jar(pin)
    assert again == jar


def test_concurrent_first_downloads_do_not_share_a_staging_file(tmp_path, monkeypatch):
    payload = b"downloaded jar"
    pin = {
        "version": "0.0.0",
        "url": "https://example.invalid/generator.jar",
        "sha256": hashlib.sha256(payload).hexdigest(),
        "sha1": hashlib.sha1(payload).hexdigest(),
    }
    destinations = []

    def download(url, dest):
        destinations.append(Path(dest))
        Path(dest).write_bytes(payload)
        if len(destinations) == 1:
            generator.ensure_generator_jar(pin)

    monkeypatch.setattr(generator, "DEFAULT_CACHE_DIR", tmp_path)
    monkeypatch.setattr(generator, "_download", download)
    jar = generator.ensure_generator_jar(pin)

    assert jar.read_bytes() == payload
    assert len(destinations) == 2
    assert destinations[0] != destinations[1]
    assert sorted(p.name for p in tmp_path.iterdir()) == [jar.name]
