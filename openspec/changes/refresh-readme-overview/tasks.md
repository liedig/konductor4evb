# Tasks

## 1. README overview

- [x] 1.1 Record the stale state before editing: run `rg -n "No schema promotion|not validated against schema content|versioned per environment|via email or API|selected by the API stage|\(\`dev\`, \`staging\`, \`prod\`\):" README.md` and confirm it reports the six stale claims, and confirm the `## How It Works` section currently holds the uncommitted Mermaid draft (`rg -n '^\`\`\`mermaid' README.md`).
- [x] 1.2 Replace the `## How It Works` fenced block with the text-art diagram from design.md (Decisions → Keep the text-art diagram), verbatim, as a `text` fence; verify that every box row inside the block has the same character width (a one-off width check over the block) and that no Mermaid fence remains in `README.md`.
- [x] 1.3 Rewrite the two paragraphs under the diagram: per-environment domain accounts through the trust role, and one API stage per configured environment (default `dev`, `staging`, `prod`) with schema writes accepted only on the first environment's stage; verify the wording against the stage-derivation and authoring-environment entries in `CHANGELOG.md`.
- [x] 1.4 Correct "Why Konductor" (schema-first contracts line) and the "Core Capabilities" bullets: console-or-API approval with email as a notice, payload validation before the `202` with `422` on violations, first-environment schema authoring with promotion (link `docs/operations/schema-promotion.md`), re-pinning on promotion, and a new System events bullet linking `docs/guides/system-events.md`; verify each changed claim against its source in the design.md claim table and that every new link target exists.
- [x] 1.5 Update "Current Scope": remove the schema-promotion and payload-validation bullets, and link the domain-teardown bullet to [D24](docs/reference/documentation-discrepancies.md#d24); verify the `rg` command from 1.1 now returns no matches.

## 2. Supporting documentation

- [x] 2.1 In `docs/README.md`, replace "lists what changed in each release" with wording that matches the changelog's own description (notable changes, not a release history); verify against the changelog's opening paragraph.
- [x] 2.2 In `docs/reference/documentation-discrepancies.md`, add a dated resolution note to D08's README paragraph (claim removed by `refresh-readme-overview`, evidence `docs/architecture/publish-validation.md`) and correct its statement that the README already linked out; keep D08's remaining items marked open, and verify no other entry changed.
- [x] 2.3 Add an `Unreleased` entry to `CHANGELOG.md` summarizing the README refresh (diagram, corrected scope claims, D08 note) and linking `openspec/changes/refresh-readme-overview/proposal.md`; verify it sits at the top of `Unreleased` and follows the existing entry style.
- [x] 2.4 Run the documentation gates: `uv run --with-requirements requirements-docs.txt mkdocs build --strict --site-dir "$(mktemp -d)"` and `uvx rumdl check README.md docs/README.md docs/reference/documentation-discrepancies.md CHANGELOG.md`; verify both pass, reporting any pre-existing findings separately from new ones.

## 3. Review

- [x] 3.1 Review the full diff against design.md: every corrected claim matches its source in the claim table, the diagram matches the design verbatim, and `git status` shows changes only in `README.md`, `docs/README.md`, `docs/reference/documentation-discrepancies.md`, `CHANGELOG.md`, and this change's directory. No Rust or `web/` files change, so the rust-reviewer and ui-reviewer droids do not apply; record that in the review note.

## Workflow follow-up

- No decision record is needed: this is an editorial correction, not a new choice about behavior or boundaries.
- Archive the change with `/opsx-archive` once the review is complete; it declares `skip_specs: true`, so no spec sync is expected.
