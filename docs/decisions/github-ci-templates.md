# Gitflow branching, three-stage CI, toolchain pinning

[Documentation home](../README.md)

- Recorded date: 2026-04-14
- Change ID: `github-ci-templates`
- Area: CI / DevEx
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Gitflow model with `develop` and `main` branches. CI workflow runs check → test → build sequentially (fail early on cheap checks). Rust toolchain pinned to stable via `rust-toolchain.toml`. `cargo-lambda` and `aws-sam-cli` installed via pip in CI. `Swatinem/rust-cache` for build caching. Makefile `check` target mirrors CI check job for local/CI parity.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-04-14-github-ci-templates/proposal.md). The recorded date above is not necessarily the archive or merge date.

## Current relevance

The Gitflow branch model was superseded by [trunk-based branching](trunk-based-branching.md). Other CI rationale remains dated context.

[Decision index](README.md)
