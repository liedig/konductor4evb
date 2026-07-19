## Context

The project has no CI pipeline, no GitHub templates, and only a `main` branch. The Makefile already defines `build`, `test`, `check`, `deploy`, and `clean` targets but `check` is missing `cargo fmt --check`. All code currently merges without automated validation.

The project is a Rust workspace (7 Lambda binary targets + 1 shared lib) built with cargo-lambda and deployed via SAM CLI. The `sam build -c` command compiles all Lambdas and packages the SAM template.

## Goals / Non-Goals

**Goals:**

- Automated checks (fmt, clippy, test, SAM build) on every PR and push to `develop` and `main`
- Fast feedback via caching and sequential job stages (fail early on cheap checks)
- Bare-bones issue and PR templates that can be iterated on
- Toolchain consistency via `rust-toolchain.toml`
- Makefile parity with CI so `make check` locally matches the CI check job

**Non-Goals:**

- Continuous deployment (CD) — no deploy step in this change
- Branch protection rules — configured in GitHub UI, not in code
- Code coverage reporting
- Matrix builds (multiple Rust versions, multiple OS)
- Integration or end-to-end tests

## Decisions

### 1. Three sequential jobs: check → test → build

The CI workflow uses three jobs with dependencies (`needs`):

| Job | What it runs | Why separate |
|-----|-------------|--------------|
| `check` | `cargo fmt --check`, `cargo check --workspace`, `cargo clippy --workspace -- -D warnings` | Fast (~30s). Catches formatting and lint issues before spending time on tests or builds |
| `test` | `cargo test --workspace` | Currently a near-no-op (no test dependencies yet) but the harness must exist for when tests are added |
| `build` | `sam validate`, `sam build -c` | Expensive (3-8 min). Proves the full Lambda compilation and SAM template are valid. Only runs if check and test pass |

**Alternative considered:** Single job running all steps sequentially. Rejected because a formatting failure shouldn't burn 5+ minutes of build time before reporting.

### 2. Trigger on PR and push to `develop` and `main`

```yaml
on:
  push:
    branches: [develop, main]
  pull_request:
    branches: [develop, main]
```

This covers:

- PR validation (before merge)
- Post-merge validation (catches broken merges, direct pushes)

Gitflow model: feature branches → `develop` (integration) → `main` (release-ready).

### 3. cargo-lambda and SAM CLI installed via pip

Both tools are available via pip and install in ~10 seconds:

```yaml
- run: pip install cargo-lambda aws-sam-cli
```

**Alternative considered:** Using dedicated GitHub Actions for each tool. Rejected — pip is simpler, faster, and fewer action dependencies to maintain.

### 4. Rust caching via `Swatinem/rust-cache`

This is the de facto standard for Rust CI caching. It caches:

- `~/.cargo/registry` (crate index + downloaded crates)
- `~/.cargo/git` (git dependencies)
- `target/` (compiled artifacts)

Cache key is derived from `Cargo.lock`, so it invalidates on dependency changes. Shared across jobs via `shared-key`.

### 5. `rust-toolchain.toml` with `channel = "stable"`

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- `rustup` reads this automatically — no CI steps needed to install the toolchain
- `stable` tracks latest stable release. Pin to a specific version (e.g., `"1.82.0"`) later if reproducibility becomes important
- Components ensure `rustfmt` and `clippy` are always present

### 6. Minimal templates with escape hatches

Issue templates use GitHub's YAML-based form format for bug reports and feature requests, with a `config.yml` that adds a blank issue option. PR template is plain markdown.

Deliberately bare-bones — structured enough to be useful, light enough that people won't skip them.

## Risks / Trade-offs

**[sam build is slow on cold cache]** → First run after a dependency change takes 5-8 minutes. Mitigated by `Swatinem/rust-cache` which brings warm builds to ~2-3 minutes. Acceptable for PR builds.

**[pip-installed cargo-lambda version drift]** → `pip install cargo-lambda` installs latest. If a breaking release lands, CI could fail. Mitigated by pinning versions in the pip install command (e.g., `cargo-lambda==1.x.x`). Start unpinned, pin if bitten.

**[No branch protection yet]** → The workflow runs checks but nothing enforces that PRs pass before merge. This is a GitHub UI setting, not a code change. Should be configured manually after the workflow is live.

**[develop branch doesn't exist yet]** → Must be created from `main` before PRs can target it. This is a one-time manual step.
