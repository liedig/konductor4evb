# Maintain the documentation

[Contributor guide](README.md)

Keep current instructions, design rationale, and historical evidence distinct.
The goal is useful context for the next human or agent, not a longer default
reading assignment.

## Choose the right home

| Content | Home |
|---|---|
| Application-team task | `docs/guides/` |
| Deployment, administration, recovery, or rollout | `docs/operations/` |
| Explanation of a system boundary or flow | `docs/architecture/` |
| Contract, identifier, status, or inventory | `docs/reference/` |
| Contributor practice and validation | `docs/development/` |
| Why a meaningful choice was made | `docs/decisions/` |
| Original proposal or superseded snapshot | `docs/history/` |
| Notable chronological change summary | Root `CHANGELOG.md` |

Maintain one primary page per topic. Link from other pages instead of copying
tables or procedures. The architecture overview is an orientation and topic map,
not the place to append every new API, workflow step, or database field.
Use the OpenAPI files for endpoint inventories.

## Before changing behavior

Read the relevant topic, capability requirement, and decision record. Follow
explicit supersession links. An old decision explains the choice at its date;
it is not a blanket instruction to reproduce obsolete behavior.

If a requirement, guide, decision, and implementation disagree, state the
conflict and scope the review. Add unresolved documentation claims to the
[discrepancy register](../reference/documentation-discrepancies.md), with links
to both claims and the evidence needed to close them. Do not silently turn
historical intent into a current guarantee.

The register is a contributor reference that stays out of the published site.
Customer-facing pages (guides, operations, architecture, and reference) do not
link to it or carry review banners. Correct the page against the
implementation, or, while a conflict stays open, state the limit on the page in
plain terms (for example, "not yet verified in a live deployment"). Close a
register entry with a dated resolution that cites the evidence.

## Record decisions without recreating a log

Create or update a decision record for a meaningful choice about behavior,
boundaries, alternatives, constraints, or trade-offs. Routine fixes and editorial
changes do not need an architectural decision just to satisfy a checklist.

A new record should include:

- A stable descriptive filename, recorded date, and change ID or commit reference.
- Status: proposed, accepted, superseded, or a clearly labeled historical record.
- Context and the problem being decided.
- Decision, alternatives considered, rationale, consequences, and limits.
- Links to the requirement, OpenSpec design/change, and validation evidence.
- Explicit “supersedes” or “superseded by” links where supported by evidence.

Keep the [decision index](../decisions/README.md) organized by topic. Preserve old
dates and IDs. Do not rewrite an old rationale to make it look as though the
later decision was always intended.

Per-handler durability records belong in the
[side-effects reference](side-effects/README.md). Record the failure walk and
follow-up IDs, not just the classification label.

## Summarize notable changes

Add a concise entry under `Unreleased` in the [changelog](../../CHANGELOG.md) for
notable user, operator, contributor, or security changes. Link to the explanation
or decision instead of copying it. Do not invent release versions, dates,
deployment completion, or acceptance evidence.

## Show commands in procedures

Installation and removal procedures show the make target for each deploy,
teardown, or step that chains several commands. Targets resolve stack outputs
where a step needs them. They also use one Region and one set of credentials
for every AWS call, which a copied sequence of commands cannot guarantee.
`scripts/tests/test_platform_make_targets.py` checks the platform deploy and
teardown targets. When a new procedure needs a chain of commands, add a target
and document the target.

Show the AWS CLI directly for single read-only checks, such as
`describe-stacks` or SES verification status, and for steps that no target
covers. Organization guardrails keep a direct `aws cloudformation deploy`:
they are a separately authorized step outside `make deploy-*` (see
[guardrail deployment](../operations/guardrails.md)).

## Move pages safely

Update active navigation and agent references in the same change. Check incoming
links, including archived OpenSpec changes. Keep a short forwarding page with
useful legacy fragments where external bookmarks or historical links need it.
Forwarding pages contain no independent technical guidance.

Validate local links and anchors, heading structure, tables, and fenced examples.
For a split or move, compare content across destinations rather than requiring
per-file equality. Run the configured Markdown checks; report inherited issues
separately from new failures. Do not run deployment examples as documentation tests.

## Publish to the documentation site

The `docs/` tree renders as a static site (MkDocs, Material theme) configured
by `mkdocs.yml` at the repository root, with a link-rewrite hook in
`docs-site/hooks.py`. A dedicated CI workflow builds the site with
`mkdocs build --strict` on pull requests—a broken internal link or anchor
fails the build. Merges to `main` deploy to GitHub Pages.

Rules that keep the gate green:

- Link between published pages with relative paths and anchors; the build
  validates them.
- Links to targets outside `docs/` (OpenSpec, templates, source files) or to
  excluded pages are allowed: the build rewrites them to GitHub blob URLs.
  The build never modifies source files.
- Heading anchors follow GitHub slug rules in both renderings (an em dash
  becomes a double hyphen). Do not "fix" anchors to another generator's
  style; the site is configured to match GitHub.
- The strategy documents (`docs/prfaq/`) and the history pages
  (`docs/history/`: original proposals, superseded snapshots, and the
  documentation baseline) stay out of the site via `exclude_docs` in
  `mkdocs.yml`. Declare exclusions only there.
  Patterns follow `.gitignore` rules: a bare filename matches at every
  depth, so anchor root-level entries with a leading `/`.
- The site navigation is declared explicitly in `mkdocs.yml` (`nav:`) and
  grouped by audience: application-team guides first, then platform
  administrator guides (install and onboard, then operate and recover), then
  concepts, reference, and contributor pages. Add every new published page to
  `nav:` in the same change; the strict build fails on a published page
  missing from it. Orientation
  pages (the home introduction, platform configuration, domain
  prerequisites, and the ACTIVE-domain page) link to primary pages rather
  than copying their procedures or tables.
- Decision records (`docs/decisions/`) are working history for the
  repository, not published documentation: they are excluded from the
  site, and links from published pages rewrite to their GitHub blob views.
- The contributor working references — Vue conventions, TypeScript
  conventions, and the side-effects rubric under `docs/development/` —
  are excluded from the site for the same reason: they serve repository
  contributors and stay reachable through GitHub.
- The [discrepancy register](../reference/documentation-discrepancies.md) is
  excluded too. Customer-facing pages describe current behavior without review
  banners or links into it; contributor pages may link to it, and the build
  rewrites those links to GitHub.
- The "Documentation moved" forwarding pages kept at old paths for
  in-repository browsing are also excluded: on the site they would only
  surface as sidebar stubs. Link to current locations in published pages.
- The site theme follows the web console: `docs/stylesheets/brand.css`
  maps the console's design tokens (`web/app/assets/css/tokens.css`) onto
  the theme's palette variables, dark is the default palette, and pages use
  the system font stack. Change the stylesheet and the tokens together; see
  the [design system](design-system.md).
- Mermaid fenced blocks render as diagrams on the site (the theme loads the
  diagram library client-side); keep diagrams as plain fenced blocks in the
  source so GitHub renders them too.
- Preview locally with `make docs-serve`. It runs the `squidfunk/mkdocs-material`
  image at the version pinned in `requirements-docs.txt` and serves the site
  with live reload at `http://localhost:8642`, listening on loopback only
  (`DOCS_PORT` changes the host port). Live reload watches `docs/` and
  `mkdocs.yml`; restart after changing `docs-site/hooks.py`. Stop it with
  Ctrl-C or `docker stop konductor-docs`. Without Docker, run `mkdocs serve`
  from a virtualenv built on `requirements-docs.txt`.
  `python3 docs-site/test_docs_serve.py` checks the target's container command.
