# Decision Log

This document tracks architectural decisions, implemented changes, and deferred technical debt for the Peaky project. It serves as a quick reference for understanding why the system is built the way it is.

## How to Use This Document

- **Implemented Decisions**: Review these when you need context on why something was built a certain way
- **Deferred Decisions**: Check here before starting new work - these are known issues that may affect your changes
- **Adding New Entries**: When archiving an OpenSpec change, add a row to the appropriate table

---

## Implemented Decisions

| Date       | Change ID                                    | Domain          | Summary                              | Key Decision                                                                                                                                                                                                                |
| ---------- | -------------------------------------------- | --------------- | ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2026-04-14 | domain-management-redesign | Domain Management | Single domain table, sync decide, workflow-owned events | Merged domains + registration requests into one table. Decide endpoint is synchronous (API GW → Lambda, no SQS). Approval workflow solely owns approval/rejection events and DDB status updates. All Step Functions use YAML + JSONata. SES notifications for approval requests and rejections. Update command is metadata-only (no status transitions). Status model: PENDING_APPROVAL → APPROVED → PROVISIONING → ACTIVE, with REJECTED, PROVISIONING_FAILED, INACTIVE. |
| 2026-04-14 | github-ci-templates | CI / DevEx | Gitflow branching, three-stage CI, toolchain pinning | Gitflow model with `develop` and `main` branches. CI workflow runs check → test → build sequentially (fail early on cheap checks). Rust toolchain pinned to stable via `rust-toolchain.toml`. `cargo-lambda` and `aws-sam-cli` installed via pip in CI. `Swatinem/rust-cache` for build caching. Makefile `check` target mirrors CI check job for local/CI parity. |