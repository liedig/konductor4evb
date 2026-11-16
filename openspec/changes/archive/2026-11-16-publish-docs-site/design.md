# Design

## Context

See proposal.md — Why. The facts that shape the approach: `docs/` holds ~104 GFM pages across nine topic directories, each with a `README.md` that acts as the section landing page; cross-references are plain relative Markdown links with occasional explicit `<a id="...">` anchors. Roughly 44 links escape the `docs/` root (into `openspec/`, repo templates, `api/`, `lib/`). The repository already uses Python for tooling (`scripts/validate-config.py`, pip installs in CI). The repository will be made public under a new `liedig` GitHub organization; the site must build and gate before that move and deploy after it.

## Goals / Non-Goals

**Goals:**

- Render the existing docs tree as a searchable static site without moving or rewriting existing pages; the only content additions are orientation pages and section framing that lead readers through setup and domain onboarding (Decision 8).
- Convert the repository's informal link hygiene into an enforced CI gate via `mkdocs build --strict`.
- Publish on every merge to `main` with no manual steps.
- Keep the site a read-only projection: all configuration for relocation (`site_url`, blob-URL base) lives in `mkdocs.yml`, and no repository tooling (OpenSpec workflow, Makefile, CI) depends on the site.

**Non-Goals:**

- Rendering `openspec/`, API definitions, or source code as site pages.
- Custom domain, versioned docs (`mike`), analytics, or i18n — all deferred deliberately.
- Moving, renaming, or rewriting existing documentation pages.
- Validating external (non-repo) links at build time; the strict gate covers internal links only.

## Decisions

### 1. MkDocs + Material, not Docusaurus/VitePress/mdBook

The corpus is plain GFM with relative `.md` links — exactly MkDocs's native input. Docusaurus's MDX parser treats prose like `{org}-*` and `{ "..." }` (both common in these docs) as JSX expressions, a build-breaking migration hazard across 104 files. Material supplies search, dark mode, and section navigation without plugins. The repo already runs Python tooling, so the pip toolchain adds no new language to the build. VitePress was the runner-up (Vue alignment with `web/`), but its search and nav maturity for docs-only sites trail Material's, and the site has no need to share code with the console.

### 2. Escaping links: build-time rewrite hook, not vendoring or root-level docs_dir

A local MkDocs hook (`docs-site/hooks.py`, `on_page_markdown`) rewrites links whose target resolves outside the published set to `https://github.com/<org>/<repo>/blob|tree/<default-branch>/<path>` (file vs directory target picked by the resolved path). Alternatives rejected: vendoring `openspec/` into the site (user decision — OpenSpec stays off the site) and `docs_dir = repo root` (requires excluding `target/`, `node_modules`, `.git`; fragile and slow). The hook operates on the in-memory page string during the build; source files are never written. The same hook also rewrites links from published pages to *excluded* pages (the strategy docs), which would otherwise fail the strict build — one mechanism covers both cases.

### 3. Exclusion via `exclude_docs`, declared once

`mkdocs.yml` excludes `prfaq/` and the two named strategy documents under `history/` using `exclude_docs` globs. The exclusion list lives in exactly one place. `history/README.md` and `history/documentation-baseline.md` remain published.

### 4. Explicit reader-ordered navigation (revised)

The first cut used automatic navigation from the `README.md`-per-directory structure. Local review showed the result was wrong for readers: auto-nav sorts sections alphabetically, so architecture deep dives led and installation sat fifth. The site now declares an explicit `nav:` in `mkdocs.yml`, ordered by reader journey: Home, Get started (platform administrators), Domains (domain teams), Publish and subscribe, Operate, Concepts, Reference, Contributing, History. Section titles are set in the nav rather than by renaming files, and `navigation.indexes` still renders each section `README.md` as its landing page.

Trade-off accepted: a hand-maintained nav can drift when pages are added. MkDocs reports published pages missing from `nav:` during the build, and the maintenance rules require adding every new published page to the nav in the same change.

### 5. One workflow, two triggers, least privilege

`.github/workflows/docs.yml` with `paths:` filters limited to `docs/**` and the site config:

- Pull requests → checkout, `pip install -r requirements-docs.txt`, `mkdocs build --strict`. No Pages permissions.
- Push to `main` → same strict build, then `configure-pages` / `upload-pages-artifact` / `deploy-pages` with `permissions: pages: write, id-token: write`, concurrency group `github-pages`, environment `github-pages`.

Deployment uses the official Pages artifact flow rather than a `gh-pages` branch push: no long-lived token, no extra branch, and the deploy target is auditable in the Actions UI.

### 6. Pinned Python dependencies

`requirements-docs.txt` pins `mkdocs-material` (which transitively pins MkDocs). Dependabot's pip ecosystem covers the file, so pins ride the existing update path instead of going stale. Trade-off accepted: transitive dependencies of MkDocs itself float; if a transitive release ever breaks the build, the pin set gets extended then.

### 7. Relocation concentrated in two config values

The GitHub blob base (`<org>/<repo>` and default branch) is declared once in `mkdocs.yml` under `extra:` and read by the hook; `site_url` sits beside it. Transferring the repository changes exactly those values and `repo_url`. Until the repository is public, the deploy job remains dormant (Pages unavailable) while the PR gate is fully active.

### 8. Orientation pages link, never copy

Reordering alone leaves gaps: no introduction, platform settings documented only as a reference table, domain prerequisites split across three sections, and no single answer to "what do I get when my domain is ACTIVE". A small set of new pages fills them: the home page becomes an introduction with two reader paths, `operations/configure-platform.md` walks through the configuration file, `guides/domain-prerequisites.md` gathers what must exist before registering, and `guides/active-domain.md` explains what provisioning delivers and what to do next. Consistent with the maintenance rules (one primary page per topic, link rather than copy), these pages explain order and purpose and link to the primary pages for procedures, fields, and tables. Existing pages stay at their paths, so no forwarding pages are needed.

## Risks / Trade-offs

- [Latent broken internal links surface as strict-build failures on first adoption] → Expected and desired; implementation includes fixing any discovered broken links in the docs as part of enabling the gate.
- [Anchor fragments on rewritten blob URLs may not survive GitHub's rendering (slug rules differ from MkDocs)] → Accept best-effort fragments; the large majority of escaping links are file-level. If a fragment matters, the target file's own header anchors usually suffice.
- [A docs page linking to an excluded strategy page breaks strict mode] → The hook rewrites those links to blob URLs (Decision 2), keeping both the exclusion and the gate intact.
- [Deploy job fails silently dormant while the repo is private] → The workflow's deploy job is gated on `github.event_name == 'push'` and Pages enablement; PR gating works today, and first deploy is verified as an explicit launch task after the public flip.
- [Strict mode also fails on MkDocs/Material version drift warnings] → Pins (Decision 6) plus Dependabot PRs make drift visible as ordinary dependency review rather than surprise breakage.

## Migration Plan

1. Land the site config, hook, dependency pin, and workflow. The PR gate activates immediately and must pass on the change's own PR.
2. Update `docs/development/documentation.md` with the new rules (strict gate, exclusion list, link conventions) and add the changelog entry.
3. Owner performs the org transfer and public flip, enables Pages (source: GitHub Actions), and merges a trivial docs change to verify first deploy end-to-end.
4. Rollback: disable the workflow or unpublish the Pages site; nothing in the repository's build or docs depends on the site existing.
