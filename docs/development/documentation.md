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

## Move pages safely

Update active navigation and agent references in the same change. Check incoming
links, including archived OpenSpec changes. Keep a short forwarding page with
useful legacy fragments where external bookmarks or historical links need it.
Forwarding pages contain no independent technical guidance.

Validate local links and anchors, heading structure, tables, and fenced examples.
For a split or move, compare content across destinations rather than requiring
per-file equality. Run the configured Markdown checks; report inherited issues
separately from new failures. Do not run deployment examples as documentation tests.
