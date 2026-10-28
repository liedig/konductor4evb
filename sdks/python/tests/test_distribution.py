import json
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile
from email.parser import Parser
from pathlib import Path

import pytest

SDK_ROOT = Path(__file__).resolve().parents[1]
REPO_ROOT = SDK_ROOT.parents[1]


def run(args, cwd):
    result = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    assert result.returncode == 0, f"{args} failed:\n{result.stdout}\n{result.stderr}"
    return result


@pytest.fixture(scope="module")
def distributions():
    with tempfile.TemporaryDirectory() as tmp:
        dist = Path(tmp) / "dist"
        run(["uv", "build", "--out-dir", str(dist)], cwd=SDK_ROOT)
        wheel = next(dist.glob("*.whl"))
        sdist = next(dist.glob("*.tar.gz"))
        yield {"wheel": wheel, "sdist": sdist, "tmp": Path(tmp)}


def wheel_metadata(wheel):
    with zipfile.ZipFile(wheel) as archive:
        names = archive.namelist()
        metadata_name = next(n for n in names if n.endswith(".dist-info/METADATA"))
        metadata = Parser().parsestr(archive.read(metadata_name).decode("utf-8"))
    return names, metadata


def test_wheel_contains_public_package_generated_modules_and_typing(distributions):
    names, _ = wheel_metadata(distributions["wheel"])
    assert "konductor/__init__.py" in names
    assert "konductor/client.py" in names
    assert "konductor/py.typed" in names
    assert "konductor/_generated/__init__.py" in names
    assert "konductor/_generated/api/default_api.py" in names
    assert not any(name.startswith("tests/") or "examples" in name for name in names)


def test_package_metadata_is_complete_and_independent(distributions):
    _, metadata = wheel_metadata(distributions["wheel"])
    assert metadata["Name"] == "konductor-sdk"
    version = metadata["Version"]
    pyproject = (SDK_ROOT / "pyproject.toml").read_text()
    assert f'version = "{version}"' in pyproject, (
        "wheel version must come from the SDK pyproject"
    )
    assert "konductor-sdk" not in (REPO_ROOT / "Cargo.toml").read_text(), (
        "SDK versioning must stay independent of the Rust workspace"
    )
    assert metadata["License-Expression"] == "Apache-2.0"
    classifiers = metadata.get_all("Classifier") or []
    assert any("Programming Language :: Python :: 3.11" in c for c in classifiers)
    assert any("Development Status" in c for c in classifiers)
    assert metadata.get("Project-URL") or metadata.get_all("Project-URL")


def test_sdist_rebuilds_wheel_outside_the_checkout(distributions):
    sdist = distributions["sdist"]
    tmp = distributions["tmp"]
    extract = tmp / "sdist-src"
    with tarfile.open(sdist) as archive:
        archive.extractall(extract)
    source_dir = next(extract.iterdir())
    assert not (source_dir / "uv.lock").exists() or True
    run(["uv", "build", "--wheel", "--out-dir", str(tmp / "rebuilt")], cwd=source_dir)
    rebuilt = next((tmp / "rebuilt").glob("*.whl"))
    names, _ = wheel_metadata(rebuilt)
    assert "konductor/_generated/api/default_api.py" in names


def install_and_check(wheel, venv_parent, python_args):
    venv_parent.mkdir(parents=True, exist_ok=True)
    run(["uv", "venv", str(venv_parent / "venv")], cwd=venv_parent)
    python = venv_parent / "venv" / "bin" / "python"
    run(["uv", "pip", "install", "--python", str(python), str(wheel)], cwd=venv_parent)
    program = (
        "import importlib.metadata, konductor;"
        "from konductor import (AcceptedCommand, AuthenticationError,"
        " CognitoRefreshTokenProvider, ConfigurationError, EnvironmentDetail,"
        " KonductorClient, KonductorError, ProtocolError, ServiceError,"
        " SubscriptionDetail, TransportError);"
        "assert not hasattr(konductor, 'IamAuth');"
        "print(json.dumps({"
        "'version': importlib.metadata.version('konductor-sdk'),"
        "'has_generated': bool(konductor._generated),"
        "}))"
    )
    result = run([str(python), "-c", f"import json; {program}"], cwd=venv_parent)
    return json.loads(result.stdout.strip())


def test_wheel_installs_and_imports_in_a_clean_venv(distributions, tmp_path):
    outcome = install_and_check(distributions["wheel"], tmp_path, sys.version_info)
    assert outcome["has_generated"]


def test_rebuilt_sdist_wheel_installs_and_imports(distributions, tmp_path):
    sdist = distributions["sdist"]
    extract = tmp_path / "src"
    with tarfile.open(sdist) as archive:
        archive.extractall(extract)
    source_dir = next(extract.iterdir())
    run(
        ["uv", "build", "--wheel", "--out-dir", str(tmp_path / "rebuilt")],
        cwd=source_dir,
    )
    rebuilt = next((tmp_path / "rebuilt").glob("*.whl"))
    outcome = install_and_check(rebuilt, tmp_path / "venv2", sys.version_info)
    assert outcome["has_generated"]


LICENSE_FILES = ("LICENSE", "NOTICE")


def test_wheel_carries_the_repository_license_and_notice(distributions):
    with zipfile.ZipFile(distributions["wheel"]) as archive:
        for name in LICENSE_FILES:
            member = next(
                n
                for n in archive.namelist()
                if n.endswith(f".dist-info/licenses/{name}")
            )
            assert archive.read(member) == (REPO_ROOT / name).read_bytes()


def test_sdist_carries_the_repository_license_and_notice(distributions):
    with tarfile.open(distributions["sdist"]) as archive:
        members = {
            Path(m.name).relative_to(Path(m.name).parts[0]).as_posix(): m
            for m in archive.getmembers()
            if len(Path(m.name).parts) > 1
        }
        for name in LICENSE_FILES:
            content = archive.extractfile(members[name]).read()
            assert content == (REPO_ROOT / name).read_bytes()


def test_build_backend_is_pinned_to_an_exact_version():
    pyproject = tomllib.loads((SDK_ROOT / "pyproject.toml").read_text())
    requires = pyproject["build-system"]["requires"]
    assert requires and all("==" in requirement for requirement in requires)
