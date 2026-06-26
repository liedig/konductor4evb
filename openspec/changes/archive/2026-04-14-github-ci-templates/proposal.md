## Why

There is no CI pipeline or GitHub contribution templates. Code changes merge without automated checks (fmt, clippy, tests, SAM build), and PRs/issues have no consistent structure. Adding these now prevents drift as the codebase grows and establishes the development workflow before onboarding contributors.

## What Changes

- Add GitHub Actions CI workflow triggered on PRs and pushes to `develop` and `main`
  - `cargo fmt --check`, `cargo check`, `cargo clippy -D warnings`
  - `cargo test --workspace`
  - `sam validate` and `sam build --beta-features -c` (with cargo-lambda)
  - Rust dependency caching for faster repeat builds
- Add `rust-toolchain.toml` pinning to stable with rustfmt and clippy components
- Add GitHub issue templates (bug report, feature request, blank escape hatch)
- Add GitHub pull request template (summary, test plan, checklist)
- Update Makefile `check` target to include `cargo fmt --check`

## Capabilities

### New Capabilities

- `ci-pipeline`: GitHub Actions CI workflow definition, triggers, job structure, caching, and toolchain setup
- `contribution-templates`: GitHub issue templates, PR template, and repository configuration for contributor workflow

### Modified Capabilities

_(none — no existing spec requirements are changing)_

## Impact

- **New files**: `.github/workflows/ci.yml`, `.github/ISSUE_TEMPLATE/bug_report.md`, `.github/ISSUE_TEMPLATE/feature_request.md`, `.github/ISSUE_TEMPLATE/config.yml`, `.github/pull_request_template.md`, `rust-toolchain.toml`
- **Modified files**: `Makefile` (add `cargo fmt --check` to `check` target)
- **Branching**: Assumes gitflow model with `develop` and `main` branches. The `develop` branch must be created before the workflow is active.
- **Dependencies**: CI requires `cargo-lambda` and `aws-sam-cli` installed via pip in the runner
