# Task-oriented documentation with retained decision context

[Decision index](README.md)

- Status: accepted for this documentation reorganization.
- Baseline: `c7e2bd6`, the user-reviewed editorial pass.
- Scope: documentation, navigation, and agent workflow instructions only.

## Context

The architecture page mixed an overview, API inventory, procedures, reference
tables, and detailed designs. The chronological decision table combined many
topics with a full handler audit. Readers and agents had to load unrelated
material, and repeated claims drifted.

## Decision

Organize documentation by reader purpose: guides, operations, architecture,
reference, development, decisions, and history. Keep the architecture overview
short and route detailed questions to focused pages.

Use a topic-indexed decision record for rationale and a concise root changelog
for chronology. Retain original change IDs, dates, alternatives, limits, and
supersession links. Put per-handler write-path records in their own contributor
reference. Keep original product proposals in history.

Agents load the affected topic, requirements, and relevant decisions rather than
the entire old log. They update the affected page. A meaningful new choice gets
a decision record; a notable change gets a changelog entry. Neither is an
unconditional task for every edit.

## Alternatives and consequences

- A changelog alone would lose the “why” and make topic discovery harder.
- Keeping the monoliths would preserve paths but keep the duplication and reading cost.
- A documentation framework is unnecessary for this Markdown repository.

Old paths retain forwarding pages and useful anchors. This adds a small
compatibility surface, but avoids rewriting archived OpenSpec evidence.
Conflicting technical claims are flagged for separate review, not reconciled
as a side effect of moving content.

This changes the packaging of the earlier [installation-guide decision](installation-guide.md),
not its aim of one clear installation starting point.
