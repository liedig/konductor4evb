# Spec Delta

## Purpose

Publishes the repository's `docs/` tree as a searchable static website built with MkDocs, with a strict link-integrity gate in CI and automated deployment to GitHub Pages, without modifying documentation source files or coupling the repository's structure to the site.

## ADDED Requirements

### Requirement: Static site build from the documentation tree

The project SHALL provide a MkDocs configuration (Material theme) that builds the `docs/` directory into a static site with built-in search, per-section navigation, and light/dark palettes. Mermaid diagram blocks in the documentation SHALL render as diagrams on the published site. The header logo SHALL use the light-background artwork under the light palette and the dark-background artwork under the dark palette. The site's visual theme SHALL follow the web console's design tokens (surface layers, text, border, and accent colors; the system font stack; the console's corner radii), with the dark palette as the default because it is the console default, and SHALL load no web fonts from third-party hosts. The build SHALL succeed under `mkdocs build --strict` so that unresolved internal links and Markdown errors fail the build. Building the site SHALL NOT modify, move, or generate files inside `docs/` or elsewhere in the repository outside the build output directory. The site's landing page SHALL be the existing `docs/README.md`, and section `README.md` files SHALL serve as section index pages.

#### Scenario: Clean strict build

- **WHEN** a contributor runs the documented site build command with the pinned dependencies
- **THEN** the build SHALL complete under `--strict` with zero warnings or errors
- **AND** every internal link between published pages SHALL resolve to a rendered page

#### Scenario: Mermaid diagrams render

- **WHEN** a published page contains a mermaid fenced code block
- **THEN** the rendered site SHALL display it as a diagram loaded with the theme's bundled diagram support
- **AND** the Markdown source SHALL remain a plain fenced block for GitHub rendering

#### Scenario: Palette-matched logo

- **WHEN** a visitor switches the site to the dark palette
- **THEN** the header logo SHALL switch to the dark-background artwork
- **AND** the light palette SHALL keep the light-background artwork

#### Scenario: Theme matches the console

- **WHEN** a visitor opens the site without a saved palette preference
- **THEN** the site SHALL render in the dark palette using the console's background, surface, border, text, and accent token values
- **AND** the light palette SHALL use the console's light token values
- **AND** the rendered pages SHALL request no fonts from Google Fonts or another third-party host

#### Scenario: Source tree untouched by builds

- **WHEN** the site is built locally or in CI
- **THEN** no file under `docs/`, `openspec/`, or any other repository path SHALL be created, modified, or deleted by the build
- **AND** a subsequent `git status` SHALL show no build-induced changes

### Requirement: Published content scope

The site SHALL publish only the `docs/` documentation tree. The strategy-genre pages — `docs/prfaq/` and the `business-requirements` and `press-release-faq` documents under `docs/history/` — the decision records under `docs/decisions/`, and the contributor working references under `docs/development/` (the Vue and TypeScript convention pages and the side-effects rubric) SHALL be excluded from the rendered site while remaining in the repository. The retired-path forwarding pages (the "Documentation moved" stubs kept at old paths for in-repository browsing) SHALL likewise be excluded from the rendered site and its navigation. OpenSpec content (`openspec/`), source code, templates, and API definitions SHALL NOT be rendered as site pages. Excluded content SHALL remain reachable through the repository itself, and exclusions SHALL be declared in one place in the site configuration.

#### Scenario: Strategy pages absent from the site

- **WHEN** the site is built and deployed
- **THEN** no page under `docs/prfaq/` and none of the named strategy documents under `docs/history/` SHALL have a rendered page, appear in navigation, or appear in site search
- **AND** the files SHALL remain readable in the repository

#### Scenario: Forwarding pages absent from the site

- **WHEN** the site is built and deployed
- **THEN** no "Documentation moved" forwarding page SHALL appear in navigation or site search
- **AND** the forwarding pages SHALL remain readable in the repository at their old paths
- **AND** a primary page that shares a filename with a forwarding page (for example `operations/installation.md` and the root `installation.md` stub) SHALL still be published

#### Scenario: OpenSpec stays out of the site

- **WHEN** a visitor browses or searches the rendered site
- **THEN** no `openspec/` content (specs, changes, archives) SHALL be present as site pages
- **AND** no decision record under `docs/decisions/` SHALL have a rendered page, nav entry, or search hit, while remaining readable in the repository and reachable from published pages through rewritten GitHub links

### Requirement: Reader-oriented navigation and onboarding paths

The site navigation SHALL be declared explicitly in the site configuration and ordered by reader journey rather than alphabetically: an introduction first, then a platform-administrator setup path (capacity planning, platform configuration, installation and its smoke test, domain-account trust, users and access, installation troubleshooting), then a domain-team path (prerequisites, registration, approval, what an ACTIVE domain provides, ongoing management), followed by publishing and subscribing, operations, concepts, reference, and contributor material. The landing page SHALL explain what Konductor is and who each path is for before directing readers into it. The site SHALL provide orientation pages for platform configuration, domain prerequisites, and the outcome of successful provisioning, and the reference index SHALL explain what each reference page covers and when a reader needs it. Orientation pages SHALL link to the primary page for each procedure, field list, or table rather than copying it. Existing pages SHALL NOT be moved or renamed to achieve the ordering.

#### Scenario: Setup leads the navigation

- **WHEN** a visitor opens the site
- **THEN** the navigation SHALL list the introduction, then the platform-administrator setup path, then the domain-team path, before operations, concepts, reference, and contributor sections
- **AND** the setup path SHALL present capacity planning, platform configuration, installation, trust deployment, and access setup in that order

#### Scenario: Domain team finds prerequisites and outcome

- **WHEN** a domain team member follows the domain path
- **THEN** a prerequisites page SHALL precede registration and name what must exist first (team membership, credentials, owner account with trust deployed, optional per-environment accounts)
- **AND** a page after approval SHALL describe what an ACTIVE domain provides and link to the next tasks

#### Scenario: New published page must join the navigation

- **WHEN** a contributor adds a published documentation page
- **THEN** the maintenance rules SHALL require adding it to the explicit navigation in the same change

### Requirement: Repository-external link resolution

Links in published pages that escape the `docs/` tree (to `openspec/`, templates, `api/`, `lib/`, or other repository paths) SHALL be rewritten at build time, in memory, to canonical GitHub blob URLs for the repository's default branch. The source Markdown files SHALL retain their original relative links so that in-repository browsing, pull-request review, and editor navigation are unchanged. The rewrite mechanism SHALL create a one-way dependency: the site adapts to the repository layout, and the repository — including the OpenSpec context mapping — SHALL remain independent of and unaware of the site. Links between published pages SHALL resolve internally and SHALL NOT be rewritten.

#### Scenario: Escaping link becomes a GitHub file view

- **WHEN** a published page contains a relative link to a path outside `docs/` (for example an archived change's evidence log)
- **THEN** the rendered site SHALL link to that file's GitHub blob view on the default branch
- **AND** the source file on disk SHALL still contain the original relative link

#### Scenario: In-scope links stay internal

- **WHEN** a published page links to another published documentation page, with or without an anchor fragment
- **THEN** the rendered site SHALL resolve it as an internal page or anchor navigation
- **AND** the strict build SHALL fail if the target does not exist

### Requirement: Automated link gate and publishing

A dedicated CI workflow SHALL build the site with `mkdocs build --strict` on pull requests and SHALL build and deploy the site to GitHub Pages on pushes to `main` only. Deployment SHALL use the GitHub Pages Actions artifact flow with least-privilege job permissions, and SHALL NOT run from pull requests or other branches. Build dependencies SHALL be pinned so local and CI builds produce the same result.

#### Scenario: Pull request with a broken internal link fails

- **WHEN** a pull request introduces a documentation change whose internal link target does not resolve
- **THEN** the docs workflow SHALL fail the strict build and report the unresolved reference
- **AND** the failure SHALL block merge under the repository's rules

#### Scenario: Merge to main publishes

- **WHEN** a documentation change is merged to `main`
- **THEN** the workflow SHALL build the site strictly and deploy the result to GitHub Pages without manual steps
- **AND** pull-request builds SHALL NOT deploy

#### Scenario: Repository relocation is a configuration change

- **WHEN** the repository moves to a new organization or the default branch's public URL changes
- **THEN** only declared configuration values (site URL and the blob-URL base) SHALL need to change
- **AND** no documentation source file or hook logic SHALL require edits
