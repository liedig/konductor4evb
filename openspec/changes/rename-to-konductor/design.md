# Design: rename-to-konductor

## Context

See proposal.md — Why. The rename is a cross-cutting sweep of 257 files, but the risk concentrates in a handful of normative strings: the lifecycle event sources (`nexus.*`), the trust-role name (`{org}-nexus-trust-role`), the stack names, and the shared crate name (`nexus-common`). Everything else is prose the compiler, `sam validate`, and the test suite can verify mechanically.

The deployment is dev-only and disposable, so there is no migration path to design: the old stack is deleted and the new one is deployed fresh.

## Goals / Non-Goals

**Goals:**

- One consistent name everywhere a reader or operator encounters the product.
- The rename is verifiable: a final grep finds no live `nexus` occurrences outside deliberately excluded history.
- The deployed dev environment ends in the same working state it is in today (seeded personas, console reachable), under the new names.

**Non-Goals:**

- No compatibility shims, dual event-source emission, or staged rollout — the consumer count is zero.
- No rewriting of archived change history (`openspec/changes/archive/**`) — dated records keep the name they were written under.
- No functional changes; any behavior drift discovered during the sweep is fixed in its own commit, not folded in.

## Decisions

### 1. Flag day, one sweep, redeploy clean

Single change, single branch, old stack deleted, new stack deployed, users re-seeded. Alternatives considered: staged rename with aliases (buys nothing with zero consumers); renaming only the product surface while keeping `nexus-` resource names (leaves permanent confusion debt and violates the point of the exercise).

### 2. The mapping table is the contract

Every occurrence must match exactly one row. If a string does not match a row, it is a decision that was missed, not an invitation to improvise.

| Old | New |
| --- | --- |
| EventBridge Nexus (display, first mention / titles) | Konductor for Amazon EventBridge |
| Nexus (short form, body text) | Konductor |
| `nexus.` (event-source prefix, e.g. `nexus.domain-management`) | `konductor.` |
| `nexus-` (resource prefix: `nexus-core`, `nexus-bus`, `nexus-domains`, Lambda/queue/log-group names) | `konductor-` |
| `{org}-nexus-trust-role`, `nexus-trust` stack | `{org}-konductor-trust-role`, `konductor-trust` |
| `nexus-common` crate / `nexus_common` imports | `konductor-common` / `konductor_common` |
| `NEXUS_*` env vars (`NEXUS_DEV_API_URL`, `NEXUS_EVENT_BUS`, `NEXUS_ROLE_ARN`) | `KONDUKTOR_*` |
| `NexusConfig` type, `$nexusConfig` provide key, `parseNexusConfig` | `KonductorConfig`, `$konductorConfig`, `parseKonductorConfig` |
| `Application=nexus` resource tag | `Application=konductor` |
| `nexus-core-stack.yaml`, `samconfig-core.toml` stack names | `konductor-core-stack.yaml`, `konductor-core` |
| `evb-nexus` GitHub repo | `konductor4evb` |
| `sliedig+nexus@example.com` (dev samconfig notification aliases) | `sliedig+konductor@example.com` |

Case forms follow mechanically: `Nexus` → `Konductor`, `NEXUS` → `KONDUKTOR`, `nexus` → `konductor`.

### 3. Spec deltas only where names are normative

Six capabilities (proposal.md — Capabilities) get requirement-level deltas because they pin names into contracts: trust-role name, SNS topic and source prefix, lifecycle event source, stack names and tags, platform resource pattern, console brand. The remaining ~10 specs mention "Nexus" only as prose; they are swept as a task, not delta'd — per-requirement MODIFIED entries for a rename carry no information.

### 4. Sweep order: identifiers before prose

Crate rename first (the compiler then flags every stale `nexus_common` import), template and event sources second (`sam validate --lint` plus the api_surfaces contract tests catch misses), prose last. Each layer commits separately so a mistake bisects cleanly.

### 5. Verify with the same live pass that caught the web UI bugs

After deploying `konductor-core`: seed users, sign in both personas in the browser, run the request → inbox → decide round-trip, and confirm a lifecycle event with source `konductor.domain-management` flows to the SNS topic. This reuses the verification script knowledge from the web UI work instead of inventing a new one.

## Risks / Trade-offs

- [A stale `nexus` reference survives in a string the compiler cannot see (event patterns, VTL templates, shell scripts)] → Final `rg -i nexus` sweep with a zero-tolerance allowlist (archived history only); live verification exercises the event path end to end.
- [The GitHub repo rename breaks local remotes and CI] → GitHub redirects cover the transition; local remotes and any hardcoded URLs are updated in the same sweep.
- [Renaming the stack orphans the old one if delete ordering is wrong] → Delete `nexus-core` only after `konductor-core` deploys and verifies; the old stack's data is disposable, so worst case is a manual console delete.
- [DynamoDB/Cognito state is lost] → Accepted explicitly: nothing deployed is worth keeping; personas are re-seeded by `make seed-users`.
- [The two parked changes (`cap-domain-count`, `audit-stream-driven-side-effects`) drift mid-rename] → Their prose is swept with everything else; their task lists do not reference product names normatively.
