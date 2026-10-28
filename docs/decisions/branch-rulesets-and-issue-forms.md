# Two branch rulesets for `main`, labels from a file, and issue forms without blank issues

[Documentation home](../README.md)

- Recorded date: 2026-10-28
- Change ID: `branch-rulesets-and-issue-forms`
- Area: CI / DevEx
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Two rulesets, because a bypass covers a whole ruleset.** `main` is protected by two repository rulesets instead of one. `main-review` requires a pull request with one approval from a code owner, dismisses stale approvals on push, requires every review conversation to be resolved, and allows only squash merges. Its single bypass actor is the repository admin role in `pull_request` mode, so a sole maintainer can merge their own pull request while direct pushes stay blocked for everyone. `main-integrity` has no bypass actors: it requires the CI checks, linear history, no force pushes, and no deletion of `main`. A ruleset's bypass skips every rule in it, so a single ruleset with an admin bypass would also let the admin skip CI. Classic branch protection was rejected because it can't be stored as a file or split in two.

**Required checks are exactly the CI jobs on `main`.** Each is bound to the GitHub Actions app, so a commit status posted under the same name can't satisfy it. The path-filtered docs build and the Droid review aren't required, because they don't report on every pull request and would leave pull requests waiting. Branches don't need to be up to date before merging, which avoids a rebase cycle on every Dependabot pull request while still requiring green checks on the pull request's own head. A new job becomes required only after the pull request that adds it merges. Requiring it earlier would block every other open pull request. A renamed job is updated on GitHub first, because the gate has no bypass.

**Labels live in a file, applied without a sync workflow.** `.github/labels.yml` is the single source of truth for label names, colors, and descriptions. It is applied with a `gh label create --force` loop, and labels it doesn't define are deleted. A label-sync workflow would add a write-scoped third-party action to a public repository for a file that rarely changes. Dependabot applies `type:chore` with `area:deps` or `area:ci`, because GitHub silently invents a label that doesn't exist.

**Four issue forms, no blank issues.** Bug report, feature request, documentation, and question forms each apply their type label and `status:needs-triage`. No form asks for a priority: outside reporters can't judge it, so maintainers set `P0`–`P3` and the area label during triage. Blank issues are disabled because they skip the bug form's redaction warning and diagnostic fields, and the question form covers anything else. The chooser links to private vulnerability reporting and to the documentation site. Maintainers who need a free-form issue use `gh issue create`.

## Evidence

Recorded directly; no OpenSpec change directory is linked. The configuration lives in `.github/rulesets/main-review.json`, `.github/rulesets/main-integrity.json`, `.github/ISSUE_TEMPLATE/`, `.github/labels.yml`, and `.github/dependabot.yml`. `scripts/tests/test_repository_config.py` checks them against each other and against `.github/workflows/ci.yml`.

[Decision index](README.md)
