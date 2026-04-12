<coding_guidelines>

## Code Style
- No code comments — write self-explanatory code with clear, intention-revealing names
- Alphabetically sorted imports
- Files under 300 lines; split when it improves clarity
- SOLID principles — separation of concerns without over-engineering
- Start simple; add complexity only when clearly required

## Testing Strategy
- Write tests for critical paths only, not every function
- AAA pattern (Arrange, Act, Assert)
- Prefer integration and behavior tests over unit test coverage metrics
- Priority test flows:
  - Domain registration and lifecycle (register, list, describe, update, delete)
  - Event publishing (schema validation, DLQ behavior)
  - Event subscription lifecycle (subscribe, list, describe, update, unsubscribe)
  - Cross-account automation (IAM/resource policies, rule creation)
- Use lightweight unit tests only where they materially reduce risk on complex logic or edge cases

## Git Workflow
- Conventional Commits: `<type>[optional scope]: <description>`
- Types: `feat`, `fix`, `refactor`, `chore`, `test`, `docs`
- Breaking changes: `!` after type/scope or `BREAKING CHANGE:` footer
- Short-lived feature branches per change (e.g., `feat-domain-management`, `fix-subscription-metrics`)

Examples:
```
feat(api): add event routing configuration endpoint
fix(auth): resolve token expiration race condition
refactor(storage): extract database connection logic
BREAKING CHANGE(api): remove deprecated v1 endpoints
```

</coding_guidelines>
