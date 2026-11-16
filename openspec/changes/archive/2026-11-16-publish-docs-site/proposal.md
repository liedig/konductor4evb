# Proposal

## Why

The documentation has grown to 100+ Markdown pages across nine topic areas — too much to stay buried in the repository tree, where there is no search, no navigation, and no readable landing experience. The repository is also about to be made public (transferred to a new `liedig` GitHub organization) as an ownership and portfolio artifact, and a rendered site is how that documentation becomes presentable. Why now: the public flip is the moment first impressions form, and the Pages URL should only ever exist at its final home.

## What Changes

- Add a MkDocs (Material theme) site configuration rendering the existing `docs/` tree as a static site with search, section navigation, and dark mode — existing pages are not rewritten and the build never modifies source Markdown.
- Add a build-time hook that rewrites *output* links escaping `docs/` (into `openspec/`, repository templates, `api/`, `lib/`) to canonical GitHub blob URLs. Repository files and the OpenSpec context mapping are untouched; the rewrite happens in memory during the build only.
- Exclude the strategy-genre pages from the rendered site: `docs/prfaq/` and the `business-requirements`/`press-release-faq` pair in `docs/history/`. The files remain in the public repository; they are simply not published as site pages.
- Add a GitHub Actions workflow that runs `mkdocs build --strict` on pull requests (a link-integrity gate) and builds + deploys to GitHub Pages on pushes to `main`.
- Update the documentation maintenance rules (`docs/development/documentation.md`) to cover the site: strict-build gate, link conventions, and the exclusion list.
- Add a changelog entry and site footer attribution; add a `NOTICE` file naming the product and author.
- Give the site a reader-ordered navigation instead of the alphabetical default: introduction first, then the platform-administrator setup path (capacity, configure, install and verify, domain-account trust, users and access), then the domain-team path (prerequisites, register, approval, what an ACTIVE domain provides), then publish/subscribe, operations, concepts, reference, and contributing last. Add a small set of orientation pages that fill gaps in those paths: an introduction on the home page, a platform configuration walkthrough, domain prerequisites, "What you get when your domain is ACTIVE", and introductions on the reference index. These pages link to the existing primary pages rather than copying their procedures or tables.

Non-goals: OpenSpec content is not rendered on the site; no custom domain (kept as a later option); no doc versioning (`mike`); existing pages are not moved or rewritten (no forwarding pages are needed).

Owner-side prerequisites (not implementation tasks): create the `liedig` org, park `konductor-io`, transfer the repository, make it public, enable GitHub Pages. The site builds and CI-gates before any of these; only deployment depends on them.

## Capabilities

### New Capabilities
- `documentation-site`: the static documentation site built from `docs/` — its content scope, its treatment of links that escape the documentation tree, its navigation, and its automated build-gate and publishing pipeline.

### Modified Capabilities

(none)

## Impact

- **New files**: `mkdocs.yml`, `docs-site/hooks.py` (link-rewrite hook), `requirements-docs.txt` (pinned Python build dependencies), `.github/workflows/docs.yml`, `NOTICE`.
- **Touched files**: `docs/development/documentation.md` (maintenance rules gain the site gate), `CHANGELOG.md` (Unreleased entry), `docs/README.md` (introduction and reader paths), section index pages (framing), and new orientation pages under `docs/operations/` and `docs/guides/`.
- **No product code, API, or template changes.** No effect on the existing CI pipeline (`ci.yml`), the Lambda workspace, or the web console build.
- **Dependencies**: MkDocs + Material theme (Python/pip, pinned). GitHub Pages hosting; Actions minutes are free for public repositories.
- **Sequencing**: everything except the Pages deploy works while the repository is still private at its current location; only `site_url` and the hook's blob-URL base change when the repository moves to the `liedig` org.
