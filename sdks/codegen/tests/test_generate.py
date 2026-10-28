import json
from pathlib import Path

import pytest

import generate


@pytest.fixture
def layout(tmp_path, monkeypatch):
    committed = tmp_path / "committed" / "_generated"
    committed.mkdir(parents=True)
    (committed / "client.py").write_text("generated v1", encoding="utf-8")
    projection = tmp_path / "projection.yaml"
    projection.write_text("projection: v1\n", encoding="utf-8")
    monkeypatch.setattr(generate, "GENERATED_PACKAGE", committed)
    monkeypatch.setattr(generate, "PROJECTION_PATH", projection)
    monkeypatch.setattr(generate, "build_projection_text", lambda: "projection: v1\n")
    monkeypatch.setattr(generate.generator_tool, "ensure_generator_jar", lambda: "jar")
    return tmp_path


def fake_generator(content):
    def run(projection_path, output_dir):
        package = Path(output_dir) / "konductor" / "_generated"
        package.mkdir(parents=True)
        (package / "client.py").write_text(content, encoding="utf-8")

    return run


def test_check_passes_when_committed_matches_fresh(layout, monkeypatch):
    monkeypatch.setattr(generate, "run_generator", fake_generator("generated v1"))
    generate.check(generate.build_projection_text())


def test_check_fails_on_stale_committed_client(layout, monkeypatch):
    monkeypatch.setattr(generate, "run_generator", fake_generator("generated v2"))
    with pytest.raises(generate.DriftError, match="generated client"):
        generate.check(generate.build_projection_text())


def test_check_fails_when_tool_inputs_changed(layout, monkeypatch):
    monkeypatch.setattr(generate, "build_projection_text", lambda: "projection: v2\n")
    monkeypatch.setattr(generate, "run_generator", fake_generator("generated v1"))
    with pytest.raises(generate.DriftError, match="stale"):
        generate.check(generate.build_projection_text())


def test_check_fails_on_nondeterministic_output(layout, monkeypatch):
    outputs = iter(["generated v1", "generated v1-alt"])

    def run(projection_path, output_dir):
        fake_generator(next(outputs))(projection_path, output_dir)

    monkeypatch.setattr(generate, "run_generator", run)
    with pytest.raises(generate.DriftError, match="deterministic"):
        generate.check(generate.build_projection_text())


def test_check_ignores_pycache_artifacts(layout, monkeypatch):
    pycache = generate.GENERATED_PACKAGE / "__pycache__"
    pycache.mkdir()
    (pycache / "client.cpython-314.pyc").write_bytes(b"bytecode")
    monkeypatch.setattr(generate, "run_generator", fake_generator("generated v1"))
    generate.check(generate.build_projection_text())


def test_check_leaves_the_working_tree_untouched(layout, monkeypatch):
    monkeypatch.setattr(generate, "run_generator", fake_generator("generated v1"))
    committed = generate.GENERATED_PACKAGE
    before = {p.name: p.read_bytes() for p in committed.rglob("*") if p.is_file()}
    generate.check(generate.build_projection_text())
    after = {p.name: p.read_bytes() for p in committed.rglob("*") if p.is_file()}
    assert before == after


def test_generator_command_comes_from_the_pin_and_sdk_version(tmp_path, monkeypatch):
    pyproject = tmp_path / "pyproject.toml"
    pyproject.write_text('[project]\nname = "konductor-sdk"\nversion = "9.8.7"\n')
    monkeypatch.setattr(generate, "PYPROJECT_PATH", pyproject)
    pin = {"version": "1.2.3", "generator": "python-pinned", "library": "lib-pinned"}
    monkeypatch.setattr(generate.generator_tool, "load_pin", lambda: pin)
    monkeypatch.setattr(
        generate.generator_tool, "ensure_generator_jar", lambda pin=None: "gen.jar"
    )
    commands = []
    monkeypatch.setattr(
        generate.subprocess, "run", lambda command, **kwargs: commands.append(command)
    )

    generate.run_generator(tmp_path / "projection.yaml", tmp_path / "out")

    command = commands[0]
    assert command[command.index("-g") + 1] == "python-pinned"
    assert command[command.index("--library") + 1] == "lib-pinned"
    assert "--additional-properties=packageVersion=9.8.7" in command


def test_sdk_version_is_read_from_the_sdk_pyproject():
    pyproject = generate.REPO_ROOT / "sdks" / "python" / "pyproject.toml"
    assert generate.PYPROJECT_PATH == pyproject
    assert f'version = "{generate.package_version()}"' in pyproject.read_text()
