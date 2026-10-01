"""Tests for the docs-site link-rewrite hook.

Run with: python3 docs-site/test_hooks.py
Uses only the standard library; no MkDocs install required.
"""

import fnmatch
import os
import sys
import tempfile
import unittest
from types import SimpleNamespace

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import hooks

BASE = "https://github.com/example/repo"
BRANCH = "main"
EXCLUDES = ["prfaq/**", "history/press-release-faq-2025-12-23.md"]


def is_excluded(docs_relative):
    return any(fnmatch.fnmatch(docs_relative, p) for p in EXCLUDES)


class MapLinkTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = self.tmp.name
        self.repo_root = root
        self.docs_dir = os.path.join(root, "docs")
        for rel in (
            "docs/guide/page.md",
            "docs/index.md",
            "docs/prfaq/secret.md",
            "docs/history/press-release-faq-2025-12-23.md",
            "openspec/specs/example/spec.md",
            "template.yaml",
        ):
            path = os.path.join(root, rel)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            open(path, "w").close()
        self.page = os.path.join(self.docs_dir, "guide", "page.md")

    def tearDown(self):
        self.tmp.cleanup()

    def link(self, target):
        return hooks.map_link(
            target,
            page_src_path=self.page,
            docs_dir=self.docs_dir,
            repo_root=self.repo_root,
            base=BASE,
            branch=BRANCH,
            is_excluded=is_excluded,
        )

    def test_internal_link_is_untouched(self):
        self.assertIsNone(self.link("../index.md"))

    def test_escaping_file_becomes_blob_url(self):
        self.assertEqual(
            self.link("../../openspec/specs/example/spec.md"),
            f"{BASE}/blob/{BRANCH}/openspec/specs/example/spec.md",
        )

    def test_escaping_directory_becomes_tree_url(self):
        self.assertEqual(
            self.link("../../openspec/specs/example"),
            f"{BASE}/tree/{BRANCH}/openspec/specs/example",
        )

    def test_repo_root_file_becomes_blob_url(self):
        self.assertEqual(
            self.link("../../template.yaml"),
            f"{BASE}/blob/{BRANCH}/template.yaml",
        )

    def test_excluded_page_becomes_blob_url(self):
        self.assertEqual(
            self.link("../prfaq/secret.md"),
            f"{BASE}/blob/{BRANCH}/docs/prfaq/secret.md",
        )
        self.assertEqual(
            self.link("../history/press-release-faq-2025-12-23.md"),
            f"{BASE}/blob/{BRANCH}/docs/history/press-release-faq-2025-12-23.md",
        )

    def test_anchor_fragment_is_preserved(self):
        self.assertEqual(
            self.link("../../template.yaml#L10"),
            f"{BASE}/blob/{BRANCH}/template.yaml#L10",
        )

    def test_external_and_fragment_targets_are_untouched(self):
        self.assertIsNone(self.link("https://example.com/x"))
        self.assertIsNone(self.link("mailto:a@example.com"))
        self.assertIsNone(self.link("#local-anchor"))

    def test_broken_in_tree_link_is_left_for_strict_validation(self):
        self.assertIsNone(self.link("../no-such-page.md"))


class OnPageMarkdownTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = self.tmp.name
        self.docs_dir = os.path.join(root, "docs")
        os.makedirs(os.path.join(self.docs_dir, "guide"))
        open(os.path.join(root, "template.yaml"), "w").close()
        open(os.path.join(self.docs_dir, "index.md"), "w").close()
        self.page_path = os.path.join(self.docs_dir, "guide", "page.md")
        open(self.page_path, "w").close()
        self.config = {
            "docs_dir": self.docs_dir,
            "exclude_docs": "prfaq/**\n",
            "extra": {"github_base": BASE, "github_branch": BRANCH},
        }
        self.page = SimpleNamespace(file=SimpleNamespace(abs_src_path=self.page_path))

    def render(self, markdown):
        return hooks.on_page_markdown(markdown, self.page, self.config, files=[])

    def test_inline_image_and_reference_links_rewrite_only_escapees(self):
        markdown = (
            "[internal](../index.md)\n"
            "![logo](../../template.yaml)\n"
            "[escaping][esc]\n\n"
            "[esc]: ../../template.yaml\n"
        )
        rendered = self.render(markdown)
        self.assertIn("[internal](../index.md)", rendered)
        self.assertIn(f"![logo]({BASE}/blob/{BRANCH}/template.yaml)", rendered)
        self.assertIn(f"[esc]: {BASE}/blob/{BRANCH}/template.yaml", rendered)


if __name__ == "__main__":
    unittest.main()
