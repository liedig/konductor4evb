# Verification

Live spot-check on the dev installation, 2026-10-20, against the deployed
stack (`make deploy-core`, UPDATE_COMPLETE). Probes used the IAM surface with
a temporary account-level publisher binding (removed afterwards; the
post-cleanup request 403s again, confirming fail-closed authorization).

| Probe | Result |
| --- | --- |
| `sales/OrderPlaced` staging, no version (pre-recording schema) | 200, real JSON Schema document, `version: 1` |
| Same, `version=1` explicit | 200, same immutable content |
| Same, `version=9` | 404 `version 9 not found in environment 'staging'` |
| Same, `environment=prod` (never promoted) | 404 `not found in environment 'prod'` |
| `billing/OrderNotification` staging + dev, no version | 200 both, `version: 3`, per-environment reads |
| `version=abc` | 400 `must be a positive integer` |
| No publisher binding | 403 `AccessDeniedException` |

Deviation from task 3.3's wording: "a current schema's latest in prod" was
not possible — both prod registries are empty (nothing has been promoted to
prod in this installation). The billing staging/dev reads cover the same
code path; prod differs only in the registry name interpolation.

The console path is covered by the Playwright specs (mocked API); no
browser-level check against the deployed OAuth stage was performed.

Reviewer passes: rust-reviewer and ui-reviewer both returned NEEDS FIX; all
findings were addressed (400/500 documented on the route, no `version: 0`
fallback, stale-response guard pins the version, distinct load-failure state,
badge-button pattern documented in `docs/development/design-system.md`).
Gates after the fixes: `make check`, 1100 workspace tests, 61 api_surfaces,
34 describe-schema, 94/94 Playwright, `pnpm lint`/`typecheck` — all clean.
