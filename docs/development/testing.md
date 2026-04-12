# Testing Guide

## TDD: Red/Green/Refactor

Follow test-driven development when implementing features:

```
┌─────────────────────────────────────────────────────────────┐
│                    RED → GREEN → REFACTOR                   │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   1. RED     Write a failing test that defines expected     │
│              behavior. Run it - it MUST fail.               │
│                                                             │
│   2. GREEN   Write minimal code to make the test pass.      │
│              No more, no less. Run test - it MUST pass.     │
│                                                             │
│   3. REFACTOR  Clean up the code (extract, rename, etc.)    │
│                while keeping the test green.                │
│                                                             │
│   Repeat for each behavior/scenario.                        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**Key principles:**
- Tests guide implementation, not the other way around
- Never change a test to make it pass - change the implementation
- Each cycle should be small (minutes, not hours)
- Commit after each green phase

**Anti-patterns to avoid:**
- Writing implementation first, then tests to match
- Writing multiple tests before any implementation
- Changing test assertions to match buggy behavior
- Skipping the refactor phase

## Test Strategy Overview

```mermaid
flowchart LR
    subgraph Workflow[Development Workflow]
        Code[Code Change] --> Check[cargo check]
        Check --> Clippy[cargo clippy]
        Clippy --> Unit[Unit Tests]
        Unit --> Integration[Integration Tests]
        Integration --> Commit[Commit]
    end
```

## Test Types

| Type | Location | Command | Purpose |
|------|----------|---------|---------|
| Unit Tests | `src/*.rs` (inline `#[cfg(test)]` modules) | `cargo test --workspace` | Test isolated logic within each crate |
| Integration Tests | `tests/*.rs` (per crate) | `cargo test --workspace` | Test cross-module behavior and AWS SDK interactions |
| Full Suite | — | `make test` | Run all tests across the workspace |

## Commands

```bash
# Run all tests across the workspace
make test

# Run tests for a specific crate
cargo test -p control-plane-common

# Run a single test by name
cargo test -p control-plane-common default_pattern_resolves_correctly

# Run tests with output visible (nocapture)
cargo test --workspace -- --nocapture

# Check compilation without running tests
make check
```

## Unit Tests

Place unit tests in a `#[cfg(test)]` module at the bottom of the source file they exercise. This keeps tests co-located with the code they verify.

```rust
pub fn event_bus_name(env: &str, org: &str, domain: &str) -> String {
    format!("{env}-{org}-{domain}-domain-bus")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_bus_name_includes_suffix() {
        let result = event_bus_name("prod", "acme", "billing");
        assert_eq!(result, "prod-acme-billing-domain-bus");
    }
}
```

**Guidelines:**
- One `mod tests` block per source file
- Use `use super::*;` to import the parent module
- Test function names should describe the behavior, not the function name
- Prefer `assert_eq!` / `assert_ne!` over bare `assert!` for better failure messages

## Integration Tests

For cross-module or AWS SDK behavior, place tests in a `tests/` directory at the crate root. Each `.rs` file in `tests/` compiles as a separate crate with access to the library's public API.

```
lib/control-plane-common/
├── src/
│   └── lib.rs
└── tests/
    └── domain_lifecycle.rs
```

```rust
use control_plane_common::NamespaceResolver;

#[test]
fn resolver_produces_consistent_naming() {
    let resolver = NamespaceResolver::default_pattern();
    let bus = resolver.event_bus_name("prod", "acme", "billing");
    let registry = resolver.schema_registry_name("prod", "acme", "billing");

    assert!(bus.starts_with("prod-acme-billing"));
    assert!(registry.starts_with("prod-acme-billing"));
}
```

## Async Tests

Lambda handlers and AWS SDK calls are async. Use `#[tokio::test]` for async test functions:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn handler_returns_success_for_valid_input() {
        let event = serde_json::json!({
            "domain_name": "billing",
            "owner_account_id": "123456789012"
        });

        let result = handle_request(event).await;
        assert!(result.is_ok());
    }
}
```

## Testing with AWS SDK Clients

Isolate AWS interactions behind traits so tests can substitute fake implementations:

```rust
#[async_trait::async_trait]
pub trait DomainStore {
    async fn put(&self, domain: &Domain) -> anyhow::Result<()>;
    async fn get(&self, id: &str) -> anyhow::Result<Option<Domain>>;
}

struct FakeDomainStore {
    domains: std::sync::Mutex<Vec<Domain>>,
}

#[async_trait::async_trait]
impl DomainStore for FakeDomainStore {
    async fn put(&self, domain: &Domain) -> anyhow::Result<()> {
        self.domains.lock().unwrap().push(domain.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<Domain>> {
        let domains = self.domains.lock().unwrap();
        Ok(domains.iter().find(|d| d.id == id).cloned())
    }
}

#[tokio::test]
async fn register_domain_persists_to_store() {
    let store = FakeDomainStore { domains: std::sync::Mutex::new(vec![]) };

    let result = register_domain(&store, "billing", "123456789012").await;

    assert!(result.is_ok());
    assert_eq!(store.domains.lock().unwrap().len(), 1);
}
```

## Error Case Testing

Use pattern matching or `assert!(result.is_err())` to verify error paths:

```rust
#[test]
fn rejects_empty_domain_name() {
    let result = validate_domain_name("");
    assert!(result.is_err());
}

#[test]
fn rejects_domain_name_with_special_chars() {
    let result = validate_domain_name("billing@v2!");

    match result {
        Err(e) => assert!(e.to_string().contains("invalid")),
        Ok(_) => panic!("expected validation error"),
    }
}
```

## Test Organization Conventions

```
crate-root/
├── src/
│   ├── lib.rs
│   ├── handler.rs        ← unit tests at bottom via #[cfg(test)]
│   ├── domain.rs         ← unit tests at bottom via #[cfg(test)]
│   └── validation.rs     ← unit tests at bottom via #[cfg(test)]
└── tests/
    ├── registration.rs   ← integration tests (public API only)
    └── lifecycle.rs      ← integration tests (public API only)
```

- Unit tests go inline in the module they test
- Integration tests go in `tests/` and only use the crate's public API
- Name test functions descriptively: `rejects_empty_domain_name` not `test_1`

## CI Considerations

- `make check` runs `cargo check` and `cargo clippy -- -D warnings` (zero warnings policy)
- `make test` runs `cargo test --workspace`
- Both must pass before merging
- SAM build (`make build`) compiles release binaries for Lambda deployment
