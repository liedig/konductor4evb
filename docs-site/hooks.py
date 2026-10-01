"""MkDocs hook: rewrite links that escape the published docs tree.

Links between published pages pass through untouched so MkDocs validates
them. Links whose target resolves outside the docs root (openspec/, repo
templates, source) or to an excluded page (strategy docs) are rewritten to
canonical GitHub blob/tree URLs. The rewrite happens on the in-memory page
during the build; source Markdown files are never modified. The repository
stays unaware of the site: all relocation config lives in mkdocs.yml
(`extra.github_base` / `extra.github_branch`).
"""

import fnmatch
import os
import re
from urllib.parse import quote

INLINE_LINK = re.compile(r"(!?\[[^\]]*\]\()([^)\s]+)((?:\s+\"[^\"]*\")?\))")
REFERENCE_DEF = re.compile(r"(^\[[^\]]+\]:\s*)(\S+)", re.MULTILINE)

_SKIP_PREFIXES = ("#", "//", "mailto:", "tel:")


def _split_target(target):
    if "#" in target:
        path, fragment = target.split("#", 1)
        return path, "#" + fragment
    return target, ""


def _exclusion_matcher(config):
    """Return a predicate over docs-relative POSIX paths for excluded pages.

    MkDocs 1.6 parses `exclude_docs` into a pathspec GitIgnoreSpec; accept a
    plain newline-separated string too so tests need no MkDocs install.
    """
    spec = config.get("exclude_docs")
    if spec is None:
        return lambda docs_relative: False
    if hasattr(spec, "match_file"):
        return spec.match_file
    patterns = [line.strip() for line in str(spec).splitlines() if line.strip()]
    return lambda docs_relative: any(fnmatch.fnmatch(docs_relative, p) for p in patterns)


def _github_url(repo_relative, is_directory, base, branch, fragment):
    kind = "tree" if is_directory else "blob"
    quoted = quote(repo_relative.replace(os.sep, "/"))
    return f"{base}/{kind}/{branch}/{quoted}{fragment}"


def map_link(target, *, page_src_path, docs_dir, repo_root, base, branch, is_excluded):
    """Return the rewritten URL for an escaping link target, else None.

    `target` is the raw Markdown link destination (relative path with an
    optional #fragment). `page_src_path` is the absolute path of the page
    being rendered. `is_excluded` is a predicate over docs-relative POSIX
    paths. Pure function: no I/O beyond os.path.isdir on the resolved target.
    """
    if not target or target.startswith(_SKIP_PREFIXES) or "://" in target:
        return None

    path, fragment = _split_target(target)
    if not path:
        return None

    resolved = os.path.normpath(os.path.join(os.path.dirname(page_src_path), path))
    if not os.path.exists(resolved):
        # Broken link: leave it untouched so MkDocs strict validation reports it.
        return None

    docs_relative = os.path.relpath(resolved, docs_dir)
    inside_docs = not docs_relative.startswith("..")

    if inside_docs and not is_excluded(docs_relative.replace(os.sep, "/")):
        return None

    repo_relative = os.path.relpath(resolved, repo_root)
    if repo_relative.startswith(".."):
        return None

    return _github_url(repo_relative, os.path.isdir(resolved), base, branch, fragment)


def _exclude_patterns(config):
    raw = config.get("exclude_docs") or ""
    return [line.strip() for line in raw.splitlines() if line.strip()]


def on_page_markdown(markdown, page, config, files):
    docs_dir = config["docs_dir"]
    repo_root = os.path.dirname(docs_dir)
    extra = config.get("extra", {})
    base = extra["github_base"].rstrip("/")
    branch = extra.get("github_branch", "main")
    is_excluded = _exclusion_matcher(config)
    page_src_path = page.file.abs_src_path

    def mapped_or_original(target):
        mapped = map_link(
            target,
            page_src_path=page_src_path,
            docs_dir=docs_dir,
            repo_root=repo_root,
            base=base,
            branch=branch,
            is_excluded=is_excluded,
        )
        return mapped if mapped else target

    def rewrite_inline(match):
        return match.group(1) + mapped_or_original(match.group(2)) + match.group(3)

    def rewrite_reference(match):
        return match.group(1) + mapped_or_original(match.group(2))

    markdown = INLINE_LINK.sub(rewrite_inline, markdown)
    return REFERENCE_DEF.sub(rewrite_reference, markdown)
