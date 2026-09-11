# Verification

## Dependency and visual baseline, 2026-09-08

- pnpm 11.20.0; Reka UI 2.10.4; Playwright 1.63.0.
- Reviewed and explicitly approved `vue-demi`'s compatibility-copy postinstall. No other build approvals added.
- Nuxt typecheck initially failed on the toast composable's component type import. Moved `BadgeTone` to `types/ui.ts`, preserving its component re-export; `pnpm typecheck` then passed.
- `pnpm lint`: zero errors, ten existing optional-prop warnings.
- `pnpm build`: passed, 22 inline scripts externalized. Existing Nitro unused-import warning and SPA prerender notice remain.
- Initial browser suite: 3/3 passed in fresh Chromium contexts with synthetic runtime configuration and no external requests. No real account sessions or service writes used.
- Baseline screenshots reviewed at 390px and 1440px. Stored locally in ignored `web/ui-evidence/refine-ui-with-reka/before/`. The narrow gallery has excessive nested padding and the existing top navigation overflows; the refresh will improve gallery padding without redesigning navigation.

## Implementation checks

- Modal/confirmation regressions first failed against the original custom wrappers, then passed against the Reka adapters. Verified focus containment, ordinary dismissal paths, opener removal fallback, long-content scrolling, Cancel-first confirmations, busy gating, and failure retry.
- Reka's overlapping hover/focus pause test failed. After the approved local lifetime change, all notification tests pass, including notifications added while paused. Window inactivity is tested through synthetic focus/blur events, not an actual OS application switch.
- Current browser suite: 18/18 passed. Typecheck and lint passed (the ten optional-prop warnings were fixed in the controls touched by this change).
- Reviewed revised desktop and narrow gallery screenshots. The gallery now avoids duplicate shell padding; application navigation is unchanged.
- Responsive tests cover 390px overlays/toasts, 200% root font size, and reduced motion. This is enlarged-text coverage, not a completed true browser-zoom check.
- Consumer inspection: domain editing, schema authoring, subscription confirmation/unsubscribe, admin domain state confirmation, and access revocation retain the same props, slots, and event contracts. Async confirmation consumers already supply busy state and control closing. No product pages or API calls were changed.
- Authenticated consumer UI inspection was unavailable in the isolated browser. Tests verified `/domains/example`, `/subscriptions`, `/admin/domains`, and `/catalog/example/Example` still redirect to login. No route guards were bypassed.
- Manual screen-reader and true browser-zoom checks remain outstanding. Desktop accessibility automation (`cua-driver`) is not installed; no new desktop permissions were granted.

## Final automated gates

- `pnpm lint`: clean, no warnings.
- `pnpm typecheck`: passed.
- `pnpm build`: passed; 22 scripts externalized. Read-only inspection confirmed no executable inline scripts in all 11 generated HTML files. Existing Nitro unused-import warning and SPA prerender notice remain.
- `pnpm test:ui`: 18/18 passed after the final wrapper changes.
- `openspec validate refine-ui-with-reka --strict`: passed.
- `git diff --check`: passed. Documented tokens and helper paths verified; gallery components remain under 300 lines.
- After screenshots, including enlarged-text overlays/toasts, reviewed and preserved under ignored `web/ui-evidence/refine-ui-with-reka/after/`.
- No backend, auth, infrastructure, or product-journey changes. The user's staged README/logo work and untracked web icon were neither edited nor staged by this implementation. Deployment is recorded below under "Live deployment".

## Manual review outcome, 2026-09-08

- True browser zoom at 200%: reviewed by the user on the live site and approved.
- Screen-reader check: explicitly waived by the user ("skip this, I don't think it's necessary at this stage"). This is a recorded gap, not a passed check — the automated suite covers the accessibility tree (roles, names, focus movement, announcements via live regions), but no real screen-reader pass was performed.
- All 17 tasks are now checked; the screen-reader waiver is annotated on task 5.2.

## Dev launcher follow-up

At the user's request, `make web-dev` now opts into opening `/design-system` through a Nuxt listen hook. AWS configuration/proxy behavior is unchanged; plain `pnpm dev` remains non-opening. The helper uses the actual listening port, launches once, and prints the URL if a desktop opener is unavailable. Six deterministic opener tests, typecheck, lint, and the Make recipe dry-run pass. No live AWS calls or real browser launches were used for this validation; existing favicon configuration was preserved.

## Live deployment, 2026-09-08

- Predeployment checks passed: six opener tests, clean lint, typecheck, and 18/18 browser tests.
- Deployed through `AWS_REGION=ap-southeast-2 rtk make deploy-web` to `s3://acme-konductor-web-444433332222-ap-southeast-2` and CloudFront distribution `E14B1M2TL5VJHP`.
- CloudFront invalidation `I56CR99L4AWLUNJ1BADH9BVUY0` completed.
- Live `/config.json` returned the expected same-origin API and Cognito runtime configuration. Live `/design-system` returned 200 and rendered the refined gallery in an isolated browser session with the new brand icon, without page or resource errors.
- True browser-zoom review was completed by the user on the live site; the screen-reader check was explicitly waived (see "Manual review outcome").
