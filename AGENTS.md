<coding_guidelines>

## Code Style

- No inline `//` comments explaining *what* code does — write self-explanatory code with clear, intention-revealing names. Exceptions: `///` doc comments on `konductor-common` public items, and brief *why*-comments for non-obvious decisions
- Import grouping: external crates first, then `konductor_common`/local modules, separated by a blank line; ordering within groups is rustfmt's job (`make check`)
- Files under 300 production lines, excluding `#[cfg(test)]` modules; split when it improves clarity
- Make invalid states unrepresentable: enums over stringly-typed values (`DomainStatus`, `Decision`), narrow purpose-built types over primitives
- Separation of concerns without over-engineering; start simple, add complexity only when clearly required
- Errors: typed variants via `thiserror` in `konductor-common` (`ControlPlaneError`) for control flow; `anyhow` with specific `.context()` only at Lambda binary edges
- No `unwrap()`/`expect()` in handler code paths — handle the `Option`/`Result`; panicking is acceptable only in tests
- No `unsafe`. If a future need arises, it requires a `// SAFETY:` comment and review sign-off

## Testing Strategy

- Write tests for critical paths only, not every function
- AAA pattern (Arrange, Act, Assert)
- Prefer integration and behavior tests over unit test coverage metrics
- When implementing the priority flows below, work test-first (red/green/refactor); test mechanics and tooling: see docs/development/testing.md
- Priority test flows:
  - Domain registration and lifecycle (register, update, list, describe, decide)
  - Event publishing (source convention, payload correctness, failed entry handling)
  - Approval workflow integration (decide handler, task token lifecycle, already-processed detection)
  - Notification generation (email body construction, HTML escaping, new account detection)
  - Shared library correctness (DDB helpers, API response mapping, serde models, namespace resolution, validation)
- When adding new capabilities (e.g., domain deletion, event subscriptions, schema validation, DLQ behavior), include their lifecycle in the priority test flows above
- Use lightweight unit tests only where they materially reduce risk on complex logic or edge cases

## Product Naming

- The product is called **EventBridge Nexus** (short form: **Nexus**)
- Resource namespace convention: `{org}-{env}-{domain}` (e.g., `acme-prod-payments`)
- Trust role: `nexus-trust-role`
- Event source: `nexus.domain-management`
- Lifecycle event bus: `nexus-bus`

## Git Workflow

- Conventional Commits: `<type>[optional scope]: <description>`
- Types: `feat`, `fix`, `refactor`, `chore`, `test`, `docs`
- Breaking changes: `!` after type/scope or `BREAKING CHANGE:` footer
- Short-lived feature branches per change (e.g., `feat-domain-management`, `fix-subscription-metrics`)
- TDD micro-commits after each green phase are fine locally; consolidate into conventional commits before review

Examples:

```text
feat(api): add event routing configuration endpoint
fix(auth): resolve token expiration race condition
refactor(storage): extract database connection logic
BREAKING CHANGE(api): remove deprecated v1 endpoints
```

## Issue Labels

Issues and PRs use a three-axis labeling system (defined in `.github/labels.yml`):

- **Priority**: `P0` (critical), `P1` (high), `P2` (medium), `P3` (low)
- **Type**: `type:bug`, `type:feature`, `type:refactor`, `type:chore`, `type:test`, `type:docs`, `type:security`
- **Area**: `area:domain-mgmt`, `area:event-streams`, `area:notifications`, `area:trust`, `area:infra`, `area:ci`, `area:deps`, `area:docs`
- **Status**: `status:needs-triage`, `status:in-progress`, `status:blocked`

New issues auto-receive `type:bug` or `type:feature` and `status:needs-triage` (these are in the issue template's static `labels` list). The area and priority dropdowns in the issue form record the submitter's suggestion in the issue body, but GitHub Issue Forms does not auto-apply those as labels. During triage, a maintainer reads the dropdown selections and applies the matching `area:*` and `P0`-`P3` labels, then removes `status:needs-triage`.

<!-- rtk-instructions v2 -->
# RTK (Rust Token Killer) - Token-Optimized Commands

## Golden Rule

**Always prefix commands with `rtk`**. If RTK has a dedicated filter, it uses it. If not, it passes through unchanged. This means RTK is always safe to use.

**Important**: Even in command chains with `&&`, use `rtk`:

```bash
# ❌ Wrong
git add . && git commit -m "msg" && git push

# ✅ Correct
rtk git add . && rtk git commit -m "msg" && rtk git push
```

## RTK Commands by Workflow

### Build & Compile (80-90% savings)

```bash
rtk cargo build         # Cargo build output
rtk cargo check         # Cargo check output
rtk cargo clippy        # Clippy warnings grouped by file (80%)
rtk tsc                 # TypeScript errors grouped by file/code (83%)
rtk lint                # ESLint/Biome violations grouped (84%)
rtk prettier --check    # Files needing format only (70%)
rtk next build          # Next.js build with route metrics (87%)
```

### Test (60-99% savings)

```bash
rtk cargo test          # Cargo test failures only (90%)
rtk go test             # Go test failures only (90%)
rtk jest                # Jest failures only (99.5%)
rtk vitest              # Vitest failures only (99.5%)
rtk playwright test     # Playwright failures only (94%)
rtk pytest              # Python test failures only (90%)
rtk rake test           # Ruby test failures only (90%)
rtk rspec               # RSpec test failures only (60%)
rtk test <cmd>          # Generic test wrapper - failures only
```

### Git (59-80% savings)

```bash
rtk git status          # Compact status
rtk git log             # Compact log (works with all git flags)
rtk git diff            # Compact diff (80%)
rtk git show            # Compact show (80%)
rtk git add             # Ultra-compact confirmations (59%)
rtk git commit          # Ultra-compact confirmations (59%)
rtk git push            # Ultra-compact confirmations
rtk git pull            # Ultra-compact confirmations
rtk git branch          # Compact branch list
rtk git fetch           # Compact fetch
rtk git stash           # Compact stash
rtk git worktree        # Compact worktree
```

Note: Git passthrough works for ALL subcommands, even those not explicitly listed.

### GitHub (26-87% savings)

```bash
rtk gh pr view <num>    # Compact PR view (87%)
rtk gh pr checks        # Compact PR checks (79%)
rtk gh run list         # Compact workflow runs (82%)
rtk gh issue list       # Compact issue list (80%)
rtk gh api              # Compact API responses (26%)
```

### JavaScript/TypeScript Tooling (70-90% savings)

```bash
rtk pnpm list           # Compact dependency tree (70%)
rtk pnpm outdated       # Compact outdated packages (80%)
rtk pnpm install        # Compact install output (90%)
rtk npm run <script>    # Compact npm script output
rtk npx <cmd>           # Compact npx command output
rtk prisma              # Prisma without ASCII art (88%)
```

### Files & Search (60-75% savings)

```bash
rtk ls <path>           # Tree format, compact (65%)
rtk read <file>         # Code reading with filtering (60%)
rtk grep <pattern>      # Search grouped by file (75%)
rtk find <pattern>      # Find grouped by directory (70%)
```

### Analysis & Debug (70-90% savings)

```bash
rtk err <cmd>           # Filter errors only from any command
rtk log <file>          # Deduplicated logs with counts
rtk json <file>         # JSON structure without values
rtk deps                # Dependency overview
rtk env                 # Environment variables compact
rtk summary <cmd>       # Smart summary of command output
rtk diff                # Ultra-compact diffs
```

### Infrastructure (85% savings)

```bash
rtk docker ps           # Compact container list
rtk docker images       # Compact image list
rtk docker logs <c>     # Deduplicated logs
rtk kubectl get         # Compact resource list
rtk kubectl logs        # Deduplicated pod logs
```

### Network (65-70% savings)

```bash
rtk curl <url>          # Compact HTTP responses (70%)
rtk wget <url>          # Compact download output (65%)
```

### Meta Commands

```bash
rtk gain                # View token savings statistics
rtk gain --history      # View command history with savings
rtk discover            # Analyze Claude Code sessions for missed RTK usage
rtk proxy <cmd>         # Run command without filtering (for debugging)
rtk init                # Add RTK instructions to CLAUDE.md
rtk init --global       # Add RTK to ~/.claude/CLAUDE.md
```

## Token Savings Overview

| Category | Commands | Typical Savings |
|----------|----------|-----------------|
| Tests | vitest, playwright, cargo test | 90-99% |
| Build | next, tsc, lint, prettier | 70-87% |
| Git | status, log, diff, add, commit | 59-80% |
| GitHub | gh pr, gh run, gh issue | 26-87% |
| Package Managers | pnpm, npm, npx | 70-90% |
| Files | ls, read, grep, find | 60-75% |
| Infrastructure | docker, kubectl | 85% |
| Network | curl, wget | 65-70% |

Overall average: **60-90% token reduction** on common development operations.
<!-- /rtk-instructions -->

</coding_guidelines>
