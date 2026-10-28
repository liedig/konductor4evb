# Capability: CI Pipeline

## Purpose

How the repository enforces quality gates automatically: a GitHub Actions workflow that runs check, test, and build stages on pull requests and protected-branch pushes, pinned toolchains, and local/CI parity through the Makefile.

## Requirements

### Requirement: CI workflow triggers

The CI workflow SHALL run on pull requests targeting `main` and on pushes to `main`. The workflow SHALL NOT run on pushes to feature branches or other branches.

#### Scenario: Pull request triggers CI

- **WHEN** a pull request is opened, synchronised, or reopened targeting `main`
- **THEN** the CI workflow SHALL execute all jobs

#### Scenario: Push to protected branch triggers CI

- **WHEN** a commit is pushed directly to `main`
- **THEN** the CI workflow SHALL execute all jobs

#### Scenario: Feature branch push does not trigger CI

- **WHEN** a commit is pushed to a branch that is not `main`
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

### Requirement: Versioned branch rulesets

The protection rules for `main` SHALL be defined as GitHub repository ruleset files under `.github/rulesets/`, so they are reviewed like code and can be applied unchanged to a new copy of the repository. The rulesets active on GitHub SHALL match those files.

#### Scenario: Ruleset change is reviewed

- **WHEN** a maintainer changes how `main` is protected
- **THEN** the change SHALL be made to a file under `.github/rulesets/` through a pull request
- **AND THEN** the updated file SHALL be applied to the repository after merge

#### Scenario: Rulesets applied when the repository goes public

- **WHEN** the repository's GitHub settings are applied, such as when it is made public
- **THEN** the rulesets under `.github/rulesets/` SHALL be applied before outside contributions are accepted

### Requirement: Pull request review gate on main

Changes to `main` SHALL arrive through pull requests approved by a code owner, with every review conversation resolved. The repository administrator MAY bypass this gate when merging a pull request, and only then, so a sole maintainer can merge their own pull requests.

#### Scenario: Outside pull request needs code-owner approval

- **WHEN** a contributor who is not a code owner opens a pull request targeting `main`
- **THEN** it SHALL NOT be mergeable until a code owner approves it and all review conversations are resolved
- **AND THEN** an approval SHALL be dismissed when new commits are pushed

#### Scenario: Administrator merges their own pull request

- **WHEN** the repository administrator merges their own pull request into `main`
- **THEN** the review gate MAY be bypassed for that merge

#### Scenario: Direct push is rejected

- **WHEN** anyone, including the repository administrator, pushes a commit directly to `main`
- **THEN** GitHub SHALL reject the push

### Requirement: Integrity gate on main

Every change merged into `main` SHALL pass the required CI checks, and `main` SHALL keep a linear history that cannot be force-pushed or deleted. No one, including the repository administrator, SHALL be able to bypass this gate.

#### Scenario: Merge waits for required checks

- **WHEN** a pull request targeting `main` has a required check that has not passed
- **THEN** it SHALL NOT be mergeable by anyone, including the repository administrator

#### Scenario: Required checks are the CI jobs

- **WHEN** the integrity gate evaluates a pull request
- **THEN** the required checks SHALL be exactly the jobs of the CI workflow on `main`
- **AND THEN** checks that may not report on every pull request, such as path-filtered workflows or reviews limited to same-repository branches, SHALL NOT be required

#### Scenario: History stays linear and protected

- **WHEN** a pull request is merged into `main`
- **THEN** it SHALL be merged as a single squashed commit
- **AND THEN** force pushes to `main` and deletion of `main` SHALL be rejected
