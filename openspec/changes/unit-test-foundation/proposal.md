## Why

`cargo test --workspace` is effectively a no-op — only 3 tests exist (namespace resolver). The CI pipeline runs tests on every PR but they don't validate any business logic. Adding unit tests for pure functions gives immediate coverage without refactoring toward trait-based mocking.

## What Changes

- Add unit tests for `control-plane-common` crate: API response/error mapping, request parsing helpers, DynamoDB attribute helpers, pagination token round-tripping, DomainStatus serde/Display
- Add unit tests for `validate_domain_name` in the domain command handler
- Add unit tests for decide-registration field validation (task_token, decidedBy, justification-on-reject)
- Extract email body generation into testable functions in notification Lambdas and add tests
- All tests follow the inline `#[cfg(test)]` convention from `docs/development/testing.md`

## Capabilities

### New Capabilities

_(none — testing is cross-cutting, not a new capability)_

### Modified Capabilities

_(none — no spec-level requirements are changing, only adding test coverage to existing implementations)_

## Impact

- **Modified files**: `lib/control-plane-common/src/api.rs`, `lib/control-plane-common/src/dynamodb.rs`, `lib/control-plane-common/src/models.rs`, `domain-management/functions/domain-command-handler/src/register.rs`, `domain-management/functions/decide-registration/src/main.rs`, `domain-management/functions/send-approval-notification/src/main.rs`, `domain-management/functions/send-rejection-notification/src/main.rs`
- **No new files**: Tests go inline per convention
- **No new dependencies**: Uses only `serde_json` (already available) for constructing test fixtures
