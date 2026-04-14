## 1. Rust Toolchain

- [x] 1.1 Create `rust-toolchain.toml` at repo root with `channel = "stable"` and components `["rustfmt", "clippy"]`

## 2. Makefile

- [x] 2.1 Add `cargo fmt --check` as the first command in the Makefile `check` target (before `cargo check` and `cargo clippy`)

## 3. CI Workflow

- [x] 3.1 Create `.github/workflows/ci.yml` with triggers on push to `develop`/`main` and pull_request targeting `develop`/`main`
- [x] 3.2 Add `check` job: install Rust toolchain (via `dtolnay/rust-toolchain` reading `rust-toolchain.toml`), set up `Swatinem/rust-cache`, run `cargo fmt --check`, `cargo check --workspace`, `cargo clippy --workspace -- -D warnings`
- [x] 3.3 Add `test` job (needs `check`): restore Rust cache, run `cargo test --workspace`
- [x] 3.4 Add `build` job (needs `test`): restore Rust cache, install `cargo-lambda` and `aws-sam-cli` via pip, run `sam validate`, run `sam build --beta-features -c`

## 4. Issue Templates

- [x] 4.1 Create `.github/ISSUE_TEMPLATE/bug_report.yml` with fields for description, steps to reproduce, expected behaviour, actual behaviour, and automatic `bug` label
- [x] 4.2 Create `.github/ISSUE_TEMPLATE/feature_request.yml` with fields for description, motivation, and automatic `enhancement` label
- [x] 4.3 Create `.github/ISSUE_TEMPLATE/config.yml` enabling blank issues

## 5. Pull Request Template

- [x] 5.1 Create `.github/pull_request_template.md` with sections for summary, test plan, and checklist (fmt, clippy, tests, docs)

## 6. Documentation

- [x] 6.1 Update decision log (`docs/development/decisions.md`) with key decisions: gitflow branching, three-stage CI (check → test → build), stable toolchain pinning, pip-based tool installation, Swatinem/rust-cache
