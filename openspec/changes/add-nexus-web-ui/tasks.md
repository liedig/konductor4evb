# Tasks: add-nexus-web-ui

## 1. Application scaffold

- [ ] 1.1 Scaffold Nuxt 4 + Tailwind CSS v4 in `web/` (static target), wired into the repo (`.gitignore`, editor config, CI build/lint job)
- [ ] 1.2 Runtime config: boot-time `config.json` fetch + typed runtime model
- [ ] 1.3 `useApi` composable: token attach, single 401→refresh→retry, typed errors; session store (Amplify Auth, sessionStorage)
- [ ] 1.4 Commit `feat(web): nuxt scaffold with runtime config and api client`

## 2. Design system

- [ ] 2.1 `tokens.css`: Tailwind v4 `@theme` semantic tokens, dark as default root (color, type, spacing, radius `rounded-lg/xl`, `shadow-sm` only)
- [ ] 2.2 Core primitives: button, input, select, badge/status pill, card, table, modal/confirm, toast, empty state
- [ ] 2.3 `/design-system` gallery route rendering every primitive (excluded from prod nav)
- [ ] 2.4 `docs/development/design-system.md`: principles (per the agreed brief), tokens, primitives, patterns, with code paths
- [ ] 2.5 Commit `feat(web): design system tokens, primitives, and documentation`

## 3. Authentication

- [ ] 3.1 `/login` page (design-system showcase) with SRP sign-in via Amplify Auth, safe error handling
- [ ] 3.2 Route middleware: auth guard, persona guard (`/admin/*` platform-only), post-login persona redirect
- [ ] 3.3 Logout + session-expiry redirect flows
- [ ] 3.4 Commit `feat(web): custom cognito login with role-gated routing`

## 4. Domain journeys

- [ ] 4.1 Owner dashboard `/domains`: owned domains with status badges, empty state with request CTA
- [ ] 4.2 Request form `/domains/new`: owning-team picker (from caller groups), full field validation mirroring the API contract, 202 handling
- [ ] 4.3 Domain detail `/domains/[name]`: overview, edit sheet (description/emails/tags only), read-only environments/account
- [ ] 4.4 Admin `/admin/domains`: full list, enable/disable with confirm
- [ ] 4.5 Commit `feat(web): domain request and management journeys`

## 5. Approvals inbox

- [ ] 5.1 `/admin` inbox: pending registrations (requester, owning team, age), approve action
- [ ] 5.2 Reject flow with mandatory justification; expired-token/conflict error surfacing
- [ ] 5.3 Commit `feat(web): approvals inbox`

## 6. Catalog and subscriptions

- [ ] 6.1 `/catalog`: entitlement-scoped browse, producer/environment filters, pagination
- [ ] 6.2 `/catalog/[producer]/[schema]`: version-per-environment, JSON content view, subscribe confirm with 202 + polling (2s, ~10s window) and failure-reason surfacing
- [ ] 6.3 `/subscriptions`: inbound list with pause/resume/unsubscribe; outbound per domain on the domain page
- [ ] 6.4 Domain page "schemas" panel via producer scope
- [ ] 6.5 Commit `feat(web): catalog discovery and subscription journeys`

## 7. Access grants

- [ ] 7.1 Domain page access panel: entitlement list, grant form (directory-picker consumer, schema picker + wildcard), delete with confirm
- [ ] 7.2 Commit `feat(web): access grant management`

## 8. Hosting and release

- [ ] 8.1 Stack: S3 bucket + CloudFront distribution (TLS, SPA fallback, security headers/CSP), outputs; `OAuthCallbackUrls` gains web origins
- [ ] 8.2 `make deploy-web`: build, sync, write `config.json` from stack outputs, invalidate
- [ ] 8.3 Verify end-to-end with seeded personas (alice: inbox-first; bob: domain-first, request → approve → catalog → grant → subscribe)
- [ ] 8.4 README web-console section; `openspec validate add-nexus-web-ui --strict` clean
- [ ] 8.5 Commit `feat(web): hosting stack and deploy workflow`
