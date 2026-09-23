# Pure-function unit tests first, trait-based mocking deferred

[Documentation home](../README.md)

- Recorded date: 2026-04-14
- Change ID: `unit-test-foundation`
- Area: Testing
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Added inline `#[cfg(test)]` modules covering all pure/extractable logic: DomainStatus serde and Display, API response helpers, DynamoDB attribute helpers, domain name validation, and email body generation (approval and rejection). Trait-based AWS SDK mocking deferred to a future change—current tests cover deterministic logic without needing network calls. Email body `format!()` extracted into standalone functions to enable testing without SES.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-04-16-unit-test-foundation/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
