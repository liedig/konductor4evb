# Tasks: rename-to-konductor

## 1. Preconditions

- [x] 1.1 Fold the authorizer caching delta into `add-nexus-web-ui` (user decision: fold in, do NOT archive — the web UI change has further work coming)
- [x] 1.2 Create the feature branch `chore-rename-to-konductor`

## 2. Code identifiers (compiler-verified)

- [x] 2.1 Rename crate `lib/nexus-common` → `lib/konductor-common` (package name, workspace members, all `nexus_common` imports); `cargo check` green
- [x] 2.2 Rename env vars `NEXUS_DEV_API_URL`, `NEXUS_EVENT_BUS`, `NEXUS_ROLE_ARN` → `KONDUKTOR_*` in Makefile, scripts, web config, and docs
- [x] 2.3 Web console: `NexusConfig` → `KonductorConfig`, `parseNexusConfig` → `parseKonductorConfig`, `$nexusConfig` → `$konductorConfig`; `pnpm typecheck` green
- [x] 2.4 Commit `refactor: rename nexus-common crate and code identifiers to konductor`

## 3. Templates and event contracts

- [x] 3.1 Rename `nexus-core-stack.yaml` → `konductor-core-stack.yaml`; rename all `nexus-` resource names, `{org}-nexus-*` patterns, `Nexus*` output keys, and `Application=nexus` tags per the design mapping table
- [x] 3.2 Rename event sources `nexus.domain-management` / `nexus.event-streams` → `konductor.*` in all emitters, the SNS forwarding rule prefix, VTL templates, and test fixtures
- [x] 3.3 `trust-stack.yaml`: role name `{org}-nexus-trust-role` → `{org}-konductor-trust-role`, stack name and tags; update `deploy-trust`/`teardown-trust` targets and output key references (`NexusFunctionRoleArn` → `KonductorFunctionRoleArn`)
- [x] 3.4 `samconfig-core.toml` (and siblings): stack names `nexus-core` → `konductor-core`, notification email aliases `sliedig+nexus@` → `sliedig+konductor@`
- [x] 3.5 `sam validate --lint`, `make check`, `cargo test --workspace`, and the `api_surfaces` contract tests all green
- [x] 3.6 Commit `refactor!: rename AWS resources and lifecycle event sources to konductor` (BREAKING: event source values change)

## 4. Prose sweep

- [x] 4.1 Docs (`README.md`, `docs/**`), `AGENTS.md` product-naming section (full form "Konductor for Amazon EventBridge", short form "Konductor"), `CLAUDE.md` if applicable, `openspec/config.yaml` glossary
- [x] 4.2 Spec prose sweep: all `openspec/specs/**` mentions of Nexus not covered by the six deltas; also the active `add-nexus-web-ui` delta prose (stays unarchived per 1.1)
- [x] 4.3 Web UI and email brand strings: login page, layout header, page titles/descriptions, notification email bodies
- [x] 4.4 Verify: `rg -i nexus` returns hits only under `openspec/changes/archive/` (historical record, intentionally untouched)
- [x] 4.5 Commit `docs: rename product to Konductor for Amazon EventBridge`

## 5. Redeploy and live verification

- [x] 5.1 Delete the disposable `nexus-core` stack and any `nexus-trust` stacks (dev only; nothing to preserve)
- [x] 5.2 Deploy `konductor-core`, `make seed-users`, `make deploy-web`; confirm the `KonductorWebUrl` (renamed) output
- [x] 5.3 Live pass per design decision 5: both personas sign in, request → inbox → decide round-trip, and a lifecycle event with source `konductor.domain-management` reaches the platform notifications topic
- [x] 5.4 Commit any deploy-fix fallout separately; stack stable

## 6. Finish

- [x] 6.1 Rename the GitHub repository `evb-nexus` → `konductor4evb`; update local remotes and any hardcoded URLs (CI, docs)
- [x] 6.2 Update decision log (`docs/development/decisions.md`) with the rename decision and the flag-day rationale
- [x] 6.3 Update `docs/development/architecture.md` resource-name references
- [x] 6.4 `openspec validate rename-to-konductor --strict` clean; commit `chore(openspec): complete rename-to-konductor tasks`
