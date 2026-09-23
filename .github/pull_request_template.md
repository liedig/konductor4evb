## Summary

<!-- What does this PR do and why? -->

## Labels

<!-- Apply at minimum one type: and one area: label. Add P0-P3 if urgency is clear. -->
- type: `type:bug` / `type:feature` / `type:refactor` / `type:chore` / `type:test` / `type:docs` / `type:security`
- area: `area:domain-mgmt` / `area:event-streams` / `area:notifications` / `area:trust` / `area:infra` / `area:ci` / `area:deps` / `area:docs`
- priority (optional): `P0` / `P1` / `P2` / `P3`

## Test Plan

<!-- How was this tested? -->

## Checklist

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] New handler write paths classified against the side-effects rubric (`docs/development/side-effects/README.md`)
- [ ] Docs updated per `docs/development/documentation.md`, including `CHANGELOG.md` for notable changes (if applicable)
