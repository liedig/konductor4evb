## Context

The codebase has 7 Lambda binary targets and 1 shared library with substantial pure logic that is untested. The testing guide (`docs/development/testing.md`) prescribes inline `#[cfg(test)]` modules for unit tests and trait-based faking for AWS SDK interactions. This change only covers pure function testing — no trait refactoring.

## Goals / Non-Goals

**Goals:**
- Make `cargo test --workspace` meaningful with 40+ tests covering core business logic
- Cover all pure utility functions in `control-plane-common`
- Cover validation logic in domain command handler and decide-registration
- Extract email body generation into testable functions
- Follow existing test conventions (inline modules, descriptive names, `assert_eq!` over `assert!`)

**Non-Goals:**
- Trait-based AWS SDK mocking (future change)
- Integration tests
- Testing Lambda handler orchestration (requires mocking)
- Code coverage targets or tooling

## Decisions

### 1. Test only pure functions, no mocking

All targets are functions that take values and return values with no AWS SDK calls. This avoids adding `mockall` or `async-trait` as dev-dependencies and keeps the change small.

### 2. Extract email body generation functions

The notification Lambdas currently build HTML strings inline in the handler. Extract these into standalone `fn build_approval_email_body(...)` and `fn build_rejection_email_body(...)` functions so they can be tested without invoking the handler.

### 3. No new dev-dependencies

All test fixtures use `serde_json::json!()` which is already a workspace dependency. No test framework beyond the standard `#[test]` harness is needed.

## Risks / Trade-offs

**[Formatting tests are fragile]** → Email body tests will assert on the HTML structure. Use `contains()` checks on key fragments rather than exact string matching to reduce brittleness.

**[DomainStatus serde tests couple to enum variant names]** → These tests document the wire format contract. If variant names change, tests should break — that's the point.
