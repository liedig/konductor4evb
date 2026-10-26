"""Tests for the `make docs-serve` target.

Run with: python3 docs-site/test_docs_serve.py
Uses only the standard library; renders the recipe with `make -n`, so
neither Docker nor MkDocs is required.
"""

import os
import re
import subprocess
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def make(*args):
    result = subprocess.run(
        ["make", "--no-print-directory", *args],
        cwd=REPO_ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    return result.stdout


def pinned_material_version():
    with open(os.path.join(REPO_ROOT, "requirements-docs.txt")) as handle:
        match = re.search(r"^mkdocs-material==(\S+)$", handle.read(), re.MULTILINE)
    assert match, "requirements-docs.txt must pin mkdocs-material"
    return match.group(1)


class DocsServeTest(unittest.TestCase):
    def test_runs_the_material_image_pinned_by_the_docs_requirements(self):
        rendered = make("-n", "docs-serve")

        self.assertIn(f"squidfunk/mkdocs-material:{pinned_material_version()}", rendered)

    def test_mounts_the_repository_root_where_the_image_expects_mkdocs_yml(self):
        rendered = make("-n", "docs-serve")

        self.assertIn(f'-v "{REPO_ROOT}":/docs', rendered)

    def test_publishes_the_preview_on_host_loopback_only(self):
        rendered = make("-n", "docs-serve")

        self.assertIn("-p 127.0.0.1:8642:8000", rendered)
        self.assertIn("http://localhost:8642", rendered)
        self.assertIn("serve --dev-addr=0.0.0.0:8000", rendered)

    def test_docs_port_overrides_only_the_host_port(self):
        rendered = make("-n", "docs-serve", "DOCS_PORT=8123")

        self.assertIn("-p 127.0.0.1:8123:8000", rendered)
        self.assertIn("http://localhost:8123", rendered)

    def test_names_the_container_so_it_can_be_stopped_by_name(self):
        rendered = make("-n", "docs-serve")

        self.assertIn("--name konductor-docs", rendered)

    def test_help_lists_the_target(self):
        self.assertIn("docs-serve", make("help"))


if __name__ == "__main__":
    unittest.main()
