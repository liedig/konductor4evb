# Capability: CI Pipeline

## Purpose

TBD

## Requirements

### Requirement: CI workflow triggers

The CI workflow SHALL run on pull requests targeting `develop` or `main` and on pushes to `develop` or `main`. The workflow SHALL NOT run on pushes to feature branches or other branches.

#### Scenario: Pull request triggers CI

- **WHEN** a pull request is opened, synchronised, or reopened targeting `develop` or `main`
- **THEN** the CI workflow SHALL execute all jobs

#### Scenario: Push to protected branch triggers CI

- **WHEN** a commit is pushed directly to `develop` or `main`
- **THEN** the CI workflow SHALL execute all jobs

#### Scenario: Feature branch push does not trigger CI

- **WHEN** a commit is pushed to a branch that is not `develop` or `main`
- **THEN** the CI workflow SHALL NOT execute

### Requirement: Code quality checks

The CI workflow SHALL run formatting, compilation, and lint checks as its first job. All checks SHALL pass before subsequent jobs execute. The checks SHALL match the Makefile `check` target so that local and CI results are consistent.

#### Scenario: Formatting check

- **WHEN** the check job runs
- **THEN** it SHALL execute `cargo fmt --check` across the workspace
- **AND THEN** fail the job if any files are not formatted according to rustfmt defaults

#### Scenario: Compilation check

- **WHEN** the check job runs
- **THEN** it SHALL execute `cargo check --workspace`
- **AND THEN** fail the job if any compilation errors exist

#### Scenario: Lint check

- **WHEN** the check job runs
- **THEN** it SHALL execute `cargo clippy --workspace -- -D warnings`
- **AND THEN** fail the job if any clippy warnings are present

#### Scenario: Check job fails early

- **WHEN** any step in the check job fails
- **THEN** the test and build jobs SHALL NOT execute

### Requirement: Test execution

The CI workflow SHALL run the workspace test suite after the check job passes.

#### Scenario: Workspace tests executed

- **WHEN** the check job has passed
- **THEN** the test job SHALL execute `cargo test --workspace`
- **AND THEN** fail the job if any test fails

#### Scenario: Test job fails before build

- **WHEN** any test fails
- **THEN** the build job SHALL NOT execute

### Requirement: SAM build validation

The CI workflow SHALL validate the SAM template and build all Lambda functions after the test job passes. This SHALL use cargo-lambda to compile Rust binaries for the `provided.al2023` Lambda runtime.

#### Scenario: SAM template validation

- **WHEN** the test job has passed
- **THEN** the build job SHALL execute `sam validate` against the SAM template
- **AND THEN** fail the job if the template contains errors

#### Scenario: SAM build compiles all Lambdas

- **WHEN** the SAM template is valid
- **THEN** the build job SHALL execute `sam build -c`
- **AND THEN** fail the job if any Lambda function fails to compile

### Requirement: Rust toolchain pinning

The repository SHALL include a `rust-toolchain.toml` file at the root that pins the Rust toolchain version and required components. CI and local development SHALL use the same toolchain automatically.

#### Scenario: Toolchain file defines stable channel

- **WHEN** a developer or CI runner enters the repository
- **THEN** `rustup` SHALL read `rust-toolchain.toml` and install the specified toolchain
- **AND THEN** the toolchain SHALL be `stable` with `rustfmt` and `clippy` components

### Requirement: Build caching

The CI workflow SHALL cache Rust compilation artifacts across runs to reduce build times. The cache SHALL invalidate when dependencies change.

#### Scenario: Cache hit on unchanged dependencies

- **WHEN** the CI workflow runs and `Cargo.lock` has not changed since the last run
- **THEN** the workflow SHALL restore cached compilation artifacts
- **AND THEN** build times SHALL be significantly reduced compared to a cold build

#### Scenario: Cache miss on dependency change

- **WHEN** the CI workflow runs and `Cargo.lock` has changed
- **THEN** the workflow SHALL perform a full build
- **AND THEN** save the new compilation artifacts to the cache

### Requirement: CI tool installation

The CI workflow SHALL install `cargo-lambda` and `aws-sam-cli` as build dependencies. These tools SHALL be installed via pip for speed and simplicity.

#### Scenario: Build tools installed in CI

- **WHEN** the build job begins
- **THEN** the workflow SHALL install `cargo-lambda` and `aws-sam-cli` via pip
- **AND THEN** both tools SHALL be available on the PATH for subsequent steps

### Requirement: Makefile CI parity

The Makefile `check` target SHALL include `cargo fmt --check` so that running `make check` locally produces the same validation as the CI check job.

#### Scenario: Local check matches CI

- **WHEN** a developer runs `make check` locally
- **THEN** it SHALL execute `cargo fmt --check`, `cargo check --workspace`, and `cargo clippy --workspace -- -D warnings` in that order
- **AND THEN** the result SHALL match what the CI check job would produce
