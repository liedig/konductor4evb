# Merged pull requests resolve referenced issues

[Documentation home](../README.md)

- Recorded date: 2026-11-03
- Change ID: `resolve-issues-on-merge`
- Area: CI / DevEx
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**A merged pull request resolves every issue its body references.** The `issues.yml` workflow runs when a pull request targeting the default branch is merged, extracts issue references from the body, comments `Resolved by #<PR> (merged into <base>)` on each, and closes it as completed. Before this, issues stayed open unless the body happened to use an exact closing keyword, and even then the only trail was a quiet timeline event. The comment makes the resolution explicit and auditable, and reopening is one click.

**Bare `#N` mentions count, not just closing keywords.** The pull request template has no "Closes" field, so keyword usage was inconsistent and keyword-only automation would have missed most associations. The accepted trade-off: a passing reference ("this reverts #45") can close an open issue that was only mentioned. That is visible in the issue's comment trail and trivially reversible, and CONTRIBUTING.md now tells contributors to reference numbers deliberately. Sidebar "Development" links and branch-name conventions were considered and rejected as detection sources; the body is the one place every contributor already writes. GitHub's GraphQL `closingIssuesReferences` was rejected because it only sees closing keywords, not bare mentions.

**Already-closed issues are commented only when keyword-referenced.** GitHub's native keyword auto-close can win the race against the workflow. A keyword reference shows explicit intent, so the reference comment lands even then, keeping the trail consistent regardless of ordering. A bare mention of an already-closed issue — likely a historical note — is left untouched.

**`pull_request_target` with no checkout keeps fork merges safe.** Under plain `pull_request`, a merged fork pull request still holds a read-only token and the comment and close calls would fail. `pull_request_target` runs in the base repository's context with a write-capable token. It is safe here because the job never checks out or executes pull request code: it reads only event-payload metadata, the body travels through an environment variable rather than `${{ }}` interpolation into the shell script (an injection vector), and the token holds only `contents: read` and `issues: write`.

**Shell plus `gh api`, not a marketplace action.** `gh` is preinstalled on GitHub runners, so the workflow adds no third-party action dependency to pin and audit. The script queries `/issues/{n}` to reject references that are actually pull requests (the endpoint resolves both), checks existing comments for the resolution marker before posting so re-runs stay idempotent, and closes with `state_reason=completed`.

## Evidence

Recorded directly; no OpenSpec change directory is linked. The automation lives in [`.github/workflows/issues.yml`](../../.github/workflows/issues.yml). `IssueResolutionWorkflowTest` in `scripts/tests/test_repository_config.py` pins its trigger, merge guard, permissions, no-checkout posture, and script invariants, and the embedded shell was validated against a stubbed `gh` covering keyword and mention detection, pull request and unknown-number skips, closed-issue handling, and re-run idempotency.

[Decision index](README.md)
