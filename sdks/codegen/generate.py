"""Regenerate the SDK's low-level client from the checked projection.

Builds the four-operation projection from the maintained OAuth contract,
runs the pinned OpenAPI Generator release, and syncs only
the generated package into sdks/python/src/konductor/_generated/. With --check,
generates into a temporary directory and fails on drift without touching the
working tree.
"""

import argparse
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

import yaml

import extractor
import generator as generator_tool

CODEGEN_DIR = Path(__file__).resolve().parent
REPO_ROOT = CODEGEN_DIR.parents[1]
PROJECTION_PATH = CODEGEN_DIR / "projection.yaml"
CONTRACT_PATH = REPO_ROOT / "api" / "openapi-oauth.yaml"
GENERATED_PACKAGE = REPO_ROOT / "sdks" / "python" / "src" / "konductor" / "_generated"
PYPROJECT_PATH = REPO_ROOT / "sdks" / "python" / "pyproject.toml"


class DriftError(Exception):
    """Committed artifacts do not match a fresh generation."""


def build_projection_text():
    oauth = yaml.safe_load(CONTRACT_PATH.read_text(encoding="utf-8"))
    projection = extractor.build_sdk_contract(oauth)
    return extractor.dump_projection(projection)


def package_version():
    with PYPROJECT_PATH.open("rb") as handle:
        return tomllib.load(handle)["project"]["version"]


def run_generator(projection_path, output_dir):
    pin = generator_tool.load_pin()
    jar = generator_tool.ensure_generator_jar(pin)
    subprocess.run(
        [
            "java", "-jar", str(jar),
            "generate",
            "-g", pin["generator"],
            "--library", pin["library"],
            "-i", str(projection_path),
            "-o", str(output_dir),
            "--package-name", "konductor._generated",
            f"--additional-properties=packageVersion={package_version()}",
        ],
        check=True,
        capture_output=True,
        text=True,
    )


def snapshot(root):
    root = Path(root)
    return {
        str(path.relative_to(root)): path.read_bytes()
        for path in sorted(root.rglob("*"))
        if path.is_file() and "__pycache__" not in path.parts
    }


def compare_trees(expected_root, actual_root, label):
    expected = snapshot(expected_root)
    actual = snapshot(actual_root)
    if expected != actual:
        only_expected = sorted(set(expected) - set(actual))
        only_actual = sorted(set(actual) - set(expected))
        changed = sorted(k for k in set(expected) & set(actual) if expected[k] != actual[k])
        raise DriftError(
            f"{label} drift: only-committed={only_expected} "
            f"only-regenerated={only_actual} changed={changed}"
        )


def check(projection_text):
    if PROJECTION_PATH.read_text(encoding="utf-8") != projection_text:
        raise DriftError(f"{PROJECTION_PATH} is stale; rerun generation")
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        projection_path = tmp / "projection.yaml"
        projection_path.write_text(projection_text, encoding="utf-8")
        run_generator(projection_path, tmp / "first")
        run_generator(projection_path, tmp / "second")
        compare_trees(
            tmp / "first" / "konductor" / "_generated",
            tmp / "second" / "konductor" / "_generated",
            "generation is not deterministic",
        )
        compare_trees(GENERATED_PACKAGE, tmp / "first" / "konductor" / "_generated", "generated client")
    print("projection and generated client are up to date")


def generate(projection_text):
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        projection_path = tmp / "projection.yaml"
        projection_path.write_text(projection_text, encoding="utf-8")
        run_generator(projection_path, tmp / "out")
        source = tmp / "out" / "konductor" / "_generated"
        PROJECTION_PATH.write_text(projection_text, encoding="utf-8")
        if GENERATED_PACKAGE.exists():
            shutil.rmtree(GENERATED_PACKAGE)
        GENERATED_PACKAGE.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(source, GENERATED_PACKAGE)
    print(f"wrote {PROJECTION_PATH.relative_to(REPO_ROOT)} and "
          f"{GENERATED_PACKAGE.relative_to(REPO_ROOT)}")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail on drift without writing")
    args = parser.parse_args(argv)
    projection_text = build_projection_text()
    if args.check:
        check(projection_text)
    else:
        generate(projection_text)


if __name__ == "__main__":
    try:
        main()
    except DriftError as error:
        print(error, file=sys.stderr)
        sys.exit(1)
