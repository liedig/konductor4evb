# Tasks

## 1. Site scaffolding

- [x] 1.1 Add `requirements-docs.txt` pinning `mkdocs-material` (and verify `pip install -r requirements-docs.txt` succeeds in a clean venv); confirm Dependabot's pip ecosystem covers the file in `.github/dependabot.yml`, adding it if absent
- [x] 1.2 Create `mkdocs.yml`: `docs_dir: docs`, Material theme with light/dark palettes, `navigation.indexes` + `navigation.sections` + search features, `site_name: Konductor for Amazon EventBridge`, `repo_url`, logo from `docs/konductor-logo-light-bg.svg`, copyright footer attribution, and `extra:` carrying the GitHub blob/tree base URL; verify `mkdocs serve` renders the landing page from `docs/README.md`
- [x] 1.3 Add `exclude_docs` globs for `prfaq/` and the `business-requirements`/`press-release-faq` documents under `history/`; verify the excluded pages produce no rendered page, nav entry, or search hit in `mkdocs build` output, while `history/README.md` and `history/documentation-baseline.md` still render

## 2. Link-rewrite hook

- [x] 2.1 Create `docs-site/hooks.py` with an `on_page_markdown` hook that rewrites relative links resolving outside the published page set to GitHub `blob/` (files) or `tree/` (directories) URLs using the base from `extra:`, preserving anchor fragments on a best-effort basis; links between published pages must pass through untouched
- [x] 2.2 Extend the hook so links from published pages to *excluded* pages (the strategy docs) rewrite to blob URLs instead of failing the strict build; verify with a link from `history/README.md` if one exists, or a temporary test page removed afterward
- [x] 2.3 Add focused tests for the hook's pure URL-mapping function (in-docs link, escaping file link, escaping directory link, excluded-page link, anchor preservation) under the repo's existing Python test conventions; verify they pass
- [x] 2.4 Verify the source-tree invariant: run a build and confirm `git status --porcelain` shows no changes under `docs/` or `openspec/` afterward

## 3. Strict gate adoption

- [x] 3.1 Run `mkdocs build --strict` and fix every pre-existing warning it reports in `docs/` (broken internal links, unlisted pages, Markdown errors), keeping fixes minimal and content-preserving; verify a clean strict build
- [x] 3.2 Update `docs/development/documentation.md` maintenance rules with the site gate (strict build must pass), the exclusion list and its rationale, and the link conventions (in-tree relative links; escaping links allowed and rewritten); verify the page itself renders on the site
- [x] 3.3 Add the `NOTICE` file (product name + author, Apache-2.0 companion) and the `CHANGELOG.md` Unreleased entry for the docs site; verify both render/parse as expected

## 4. CI workflow

- [x] 4.1 Create `.github/workflows/docs.yml`: PR trigger (paths `docs/**`, `mkdocs.yml`, `docs-site/**`, `requirements-docs.txt`) running checkout → pip install → `mkdocs build --strict` with no Pages permissions; verify it goes green on this change's own PR
- [x] 4.2 Add the `main`-push job to the same workflow: strict build plus `configure-pages`/`upload-pages-artifact`/`deploy-pages` with `permissions: pages: write, id-token: write`, `environment: github-pages`, and a `github-pages` concurrency group; verify by inspection of the workflow run on merge (deploy expected to fail gracefully or remain unavailable until Pages is enabled — record the observed behavior)
  - Observed on merge (`e65cb3d`, run [36803692779](https://github.com/sliedig/konductor4evb/actions/runs/36803692779), 2026-10-01): checkout, install, hook tests, and `mkdocs build --strict` passed on `main`; `configure-pages` failed with "Resource not accessible by integration" because Pages is not enabled, so the run is red and the deploy job was skipped. Nothing was published. Expected to clear at task 5.2.

## 5. Launch (owner-only steps, after org transfer and public flip)

- [x] 5.1 Update `site_url`, `repo_url`, and the `extra:` blob base in `mkdocs.yml` for the `liedig` org location; verify the strict build still passes and rewritten links point at the new org
- [ ] 5.2 Enable GitHub Pages (source: GitHub Actions) on the transferred public repository; merge a trivial docs change and verify the deployed site loads at the Pages URL, search works, and an escaping link lands on the correct GitHub blob view
- [ ] 5.3 Record the launch in the change (evidence note appended to this file or the PR): deployed URL, first successful deploy run id, and the Pages settings used

## 6. Preview corrections (local site review)

- [x] 6.1 Exclude the ten "Documentation moved" forwarding pages from the site via `exclude_docs` so they stop surfacing as sidebar entries; verify no rendered page, nav entry, or search hit for any stub while the files remain in the repository
- [x] 6.2 Serve the dark-background logo artwork under the slate palette (copy `konductor-logo-dark-bg.svg` into `docs/`, add the brand stylesheet and `extra_css`); verify the swap in rendered output and the light palette keeping the light-bg logo
- [x] 6.3 Enable Mermaid rendering via `pymdownx.superfences` custom fences; verify a rendered architecture page contains diagram markup and the theme's mermaid bundle
- [x] 6.4 Re-run the hook tests and a clean strict build; verify the source-tree invariant and update this change's spec delta to cover the three behaviors
- [x] 6.5 Exclude the decision records under `docs/decisions/` from the published site; verify no rendered page, nav entry, or search hit for any decision record while published-page links to them rewrite to GitHub blob URLs
- [x] 6.6 Exclude the contributor working references under `docs/development/` (Vue conventions, TypeScript conventions, and the side-effects rubric) from the published site; verify no rendered pages, nav entries, or search hits while the files remain in the repository

## 7. Reader-oriented structure

- [x] 7.0 Anchor the root forwarding-stub exclusions with a leading `/`: bare `.gitignore`-style names also excluded `operations/installation.md`, `operations/service-quotas.md`, `operations/trust-stack-deployment.md`, and `guides/publishing-events.md`; verify all four render and inbound links stay internal
- [x] 7.1 Rework `docs/README.md` into an introduction (what Konductor is, the two account types, who does what) with a platform-administrator path and a domain-team path; keep the discrepancy-register pointer; verify every link resolves under the strict build
- [x] 7.2 Add `docs/operations/configure-platform.md`: a walkthrough of `konductor-platform.toml` (create, choose fixed-at-setup values, set updateable values, validate, adopt from an existing stack) that links to the configuration contract for syntax and bounds instead of copying the table; verify it renders and links resolve
- [x] 7.3 Add `docs/guides/domain-prerequisites.md` (team group and user, credentials, owner account with trust deployed, optional per-environment accounts, naming choice, notifications) and `docs/guides/active-domain.md` (what provisioning creates, where it lives, how to find it, the owner binding, and next steps), both linking to primary pages; verify they render and links resolve
- [x] 7.4 Frame the section index pages: reference intro with "what it is and when you need it" per page; Get started and Domains ordering in the operations and guides indexes; verify the strict build
- [x] 7.5 Declare the explicit reader-ordered `nav:` in `mkdocs.yml` covering every published page; verify the rendered sidebar order and that the build reports no published page missing from the nav
- [x] 7.6 Update `docs/development/documentation.md` (new published pages join the nav) and the `CHANGELOG.md` site entry; verify hook tests, strict build, and `openspec validate publish-docs-site --strict`

## 8. Console-matched theme

- [x] 8.1 Map the console design tokens (`web/app/assets/css/tokens.css`) onto Material's palette variables in `docs/stylesheets/brand.css` for both palettes; make dark the default palette, switch to the system font stack (`font: false`), and use the console favicon; verify the rendered pages in both palettes against console screenshots, and that built HTML references no Google Fonts
- [x] 8.2 Record the sync rule (site theme mirrors console tokens; change both together) in `docs/development/documentation.md` and `docs/development/design-system.md`; verify the strict build and hook tests
