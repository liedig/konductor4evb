# Design: rename-to-konductor

## Context

See proposal.md — Why. The rename was a cross-cutting sweep of 257 files, with risk concentrated in a handful of normative strings: the lifecycle event sources (`konductor.*` today), the trust-role name (`{org}-konductor-trust-role`), the stack names, and the shared crate name (`konductor-common`). Everything else was prose the compiler, `sam validate`, and the test suite verified mechanically.

The deployment was dev-only and disposable, so there was no migration path to design: the old stack was deleted and the new one deployed fresh. (The previous name is preserved in git history; this record deliberately uses only the new naming.)

## Goals / Non-Goals

**Goals:**

- One consistent name everywhere a reader or operator encounters the product.
- The rename is verifiable: a final grep finds zero occurrences of the old name anywhere in the repository.
- The deployed dev environment ends in the same working state it was in before (seeded personas, console reachable), under the new names.

**Non-Goals:**

- No compatibility shims, dual event-source emission, or staged rollout — the consumer count is zero.
- No functional changes; any behavior drift discovered during the sweep is fixed in its own commit, not folded in.

## Decisions

### 1. Flag day, one sweep, redeploy clean

Single change, single branch, old stack deleted, new stack deployed, users re-seeded. Alternatives considered: staged rename with aliases (buys nothing with zero consumers); renaming only the product surface while keeping resource names (leaves permanent confusion debt and violates the point of the exercise).

### 2. The mapping table is the contract

Every occurrence matched exactly one row. A string matching no row was a missed decision, not an invitation to improvise. The resulting naming contract (all current names):

| Surface | Name |
| --- | --- |
| Display, first mention / titles | Konductor for Amazon EventBridge |
| Short form, body text | Konductor |
| Event sources | `konductor.domain-management`, `konductor.event-streams` (rule prefix `konductor.`) |
| Resource prefix | `konductor-` (`konductor-core`, `konductor-bus`, `konductor-domains`, Lambda/queue/log-group names) |
| Trust role / trust stack | `{org}-konductor-trust-role`, `konductor-trust` |
| Shared crate | `konductor-common` / `konductor_common` |
| Env vars | `KONDUKTOR_DEV_API_URL`, `KONDUKTOR_EVENT_BUS`, `KONDUKTOR_ROLE_ARN` |
| Web config | `KonductorConfig`, `$konductorConfig`, `parseKonductorConfig` |
| Resource tag | `Application=konductor` |
| Template / stack | `konductor-core-stack.yaml`, `konductor-core` |
| GitHub repo | `konductor4evb` |
| Dev notification aliases | `sliedig+konductor@example.com` |

Case forms followed mechanically: `Konductor`, `KONDUKTOR`, `konductor`.

### 3. Spec deltas only where names are normative

Six capabilities (proposal.md — Capabilities) got requirement-level deltas because they pin names into contracts: trust-role name, SNS topic and source prefix, lifecycle event source, stack names and tags, platform resource pattern, console brand. The remaining specs mentioned the product only as prose; they were swept as a task, not delta'd — per-requirement MODIFIED entries for a rename carry no information.

### 4. Sweep order: identifiers before prose

Crate rename first (the compiler then flagged every stale import), template and event sources second (`sam validate --lint` plus the api_surfaces contract tests caught misses), prose last. Each layer committed separately so a mistake bisects cleanly.

### 5. Verify with the same live pass that caught the web UI bugs

After deploying `konductor-core`: seeded users, signed in both personas in the browser, ran the request → inbox → decide round-trip, and confirmed lifecycle events with source `konductor.domain-management` flow to the SNS topic. This reused the verification script knowledge from the web UI work instead of inventing a new one.

## Risks / Trade-offs

- [A stale reference survives in a string the compiler cannot see (event patterns, VTL templates, shell scripts)] → Final `rg -i` sweep with a zero-tolerance gate (after implementation the user extended the gate to archived history as well — absolute zero); live verification exercised the event path end to end.
- [The GitHub repo rename breaks local remotes and CI] → GitHub redirects covered the transition; the local remote was updated in the same sweep (no hardcoded URLs existed in CI or docs).
- [Renaming the stack orphans the old one if delete ordering is wrong] → the old stack was deleted first and the new one deployed fresh; the old stack's data was disposable (one S3-empty hiccup on delete, resolved by emptying the web bucket).
- [DynamoDB/Cognito state is lost] → Accepted explicitly: nothing deployed was worth keeping; personas were re-seeded by `make seed-users`.
- [The parked changes (`cap-domain-count`, `audit-stream-driven-side-effects`) drift mid-rename] → Their prose was swept with everything else; their task lists do not reference product names normatively.
