# Design

## Context

See proposal.md (Why) for the motivation. The facts that shape the approach:

- The root `README.md` is outside the MkDocs `docs_dir`, so the strict site build does not check it. `docs/README.md` and `docs/reference/documentation-discrepancies.md` are published and are checked.
- `.rumdl.toml` disables the line-length rule because of diagrams and tables, so a wide text-art block passes Markdown linting.
- The working copy of `README.md` holds an uncommitted Mermaid draft of the diagram. Several of its labels wrap across source lines, and the maintainer chose to keep the text-art style for now.
- Documentation maintenance rules (`docs/development/documentation.md`) say the README links to primary pages instead of copying their detail. Conflicts go to the discrepancy register rather than being silently resolved.

No handler write paths are added or changed, so the side-effects rubric does not apply.

## Goals / Non-Goals

**Goals:**

- Every claim in the README overview can be traced to a shipped requirement, a topic page, or the changelog.
- The refreshed diagram keeps the original layout so readers who know it can spot what moved.

**Non-Goals:**

- Rewording README sections that are still accurate (Documentation table, Web Console, Technology).
- Re-verifying claims the discrepancy register already tracks as open, such as the "12+ resources" figure.

## Decisions

### Keep the text-art diagram and extend it

The diagram stays a `text` fenced block with the same box-drawing style. A fourth box, System events, sits under Event streams and is reached by an "upgrade notices" connector. Box titles move to the left edge so the vertical connectors no longer cut through them, and every row is padded to the same width.

Alternatives considered: Mermaid in the README (simplified) plus the architecture page (detailed), or replacing the diagram with a link to the architecture overview. The maintainer deferred both; they stay possible later without undoing this change.

Target diagram (the apply phase uses this verbatim unless a claim check below fails):

```text
┌── CONTROL PLANE ─────────────────────────────────────────────────────────────────────┐
│  OAuth + IAM APIs and web console · grant engine (bindings + entitlements) ·         │
│  audit on every mutation · platform metadata revisions · revocation worker +         │
│  hourly sweep · operator alarms                                                      │
└─────────────────────────────┬─────────────────────────────────────────┬──────────────┘
                              │ authorizes every command                │ disables streams
                              ▼                                         ▼ that lose coverage
┌── DOMAIN MANAGEMENT ────────┴──────────┐   ┌── EVENT STREAMS ─────────┴──────────────┐
│  register → approve → provision every  │   │  schemas: authored in the first env,    │
│  environment atomically, each in its   │   │  appends compatibility-gated, then      │
│  pinned account: buses, registries,    │──▶│  promoted env → env with the same       │
│  DLQs, and monitoring in the platform  │   │  version numbers                        │
│  and domain accounts                   │   │                                         │
│                                        │   │  publish: one call, authorized and      │
│  approval in the console or API; email │   │  validated against the latest schema    │
│  only notifies the approvers           │   │  before the 202, then durably queued    │
│                                        │   │                                         │
│  lifecycle events and owner            │   │  subscribe: catalog shows only what     │
│  notifications on every transition     │   │  entitlements cover; per-env rules      │
│                                        │   │  deliver cross-account; re-pinned on    │
│  teardown reuses the deprovision       │   │  promotion; pause/resume per env        │
│  workflow (make teardown-domain)       │   │                                         │
└────────────────────────────────────────┘   └──────────────────────────┬──────────────┘
                                                                        │ upgrade notices
                                                                        ▼
                                             ┌── SYSTEM EVENTS ─────────┴──────────────┐
                                             │  subscription upgrades publish to the   │
                                             │  system-event bus and route to each     │
                                             │  domain's own SNS topic (SQS, Lambda,   │
                                             │  or email); opt-in per installation     │
                                             └─────────────────────────────────────────┘
```

### Check each corrected claim against a named source

Each corrected claim links to, or is checked against, one primary source. The README links to topic pages instead of repeating their detail.

| README claim | Source of truth |
|---|---|
| Schemas authored in the first environment, then promoted | `docs/architecture/schemas.md` (Authoring environment and promotion); CHANGELOG promotion and breaking-change entries |
| Payload validated before `202`, `422` on violations | `docs/architecture/publish-validation.md` sections 2 and 4 |
| Approval in the console or API; email only notifies | `send-approval-request-notification` (email links to the console's Pending approvals); `POST /domains/registrations/decide` exists only in `api/openapi-oauth.yaml` |
| Stages follow the configured environment list | CHANGELOG entry on stages derived at deploy time |
| Per-environment domain accounts | CHANGELOG `environmentAccounts` entry; `docs/decisions/per-environment-domain-accounts.md` |
| Re-pinning on promotion | `docs/architecture/subscriptions.md` (widen, drain, narrow) |
| System events, opt-in | `docs/guides/system-events.md`; CHANGELOG entry (`SystemEventEmissionEnabled`, default off) |
| Teardown limitation | D24 in the discrepancy register |

### Resolve the README part of D08 without closing D08

D08 bundles several stale summaries. This change adds a dated resolution note to its README paragraph (claim removed, evidence: this change and `publish-validation.md`) and corrects its wording, which said the README already linked out. D08's other items stay open. Closing D08 entirely would overstate what was reviewed.

## Risks / Trade-offs

- [Box-drawing glyphs render at uneven widths in some fonts or East Asian locales] → Same risk the original diagram carried; the layout keeps one glyph per column and is checked in a monospace preview on GitHub.
- [The diagram drifts again as features ship] → The README bullets link to topic pages, so only the one-line summaries need updating. Moving to Mermaid remains the longer-term option.
- [Replacing the uncommitted Mermaid draft discards maintainer work] → The maintainer asked to keep the text-art diagram. The Mermaid draft is recorded in this session's exploration and can be revived later.
