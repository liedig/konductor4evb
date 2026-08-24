# Design: add-konductor-web-ui

## Context

The OAuth API surface, Cognito pool (with `platform-admin`/`platform-auditor` global groups and dynamic team groups), and all read/write contracts for the target journeys already exist or are delivered by enable-oauth-domain-management. No frontend exists today. See proposal.md — Why.

## Visual identity (product brief, agreed 2026-08-11)

**Clean, airy, professional** — Konductor balances enterprise credibility with approachable clarity. The interface avoids visual noise while maintaining enough density for serious workflows.

Key aesthetic principles:

- **Layered depth**: Background → Cards → Interactive elements
- **Generous whitespace**: Breathing room reduces cognitive load
- **Color with intent**: Status colors carry meaning (green=ready, blue=active, red=urgent)
- **Consistent radius**: All rounded elements use `rounded-lg` or `rounded-xl`
- **Subtle shadows**: Only `shadow-sm` for elevation, never heavy drop shadows

Dark-from-day-one: dark is the default theme (not an inversion pass later), with tokens defined once as CSS variables.

## Goals / Non-Goals

**Goals:** one deployable console covering the four journey areas; a design system documented in `docs/` with tokens implemented once and consumed everywhere; session security appropriate for an internal enterprise tool.

**Non-goals:** SSR/SEO (gated internal app — static generation suffices); real-time push (polling is the agreed pattern); platform configuration screens (parked); schema lifecycle UI beyond what the API exposes (#11); the EventCatalog integration (#10); MFA enrollment UX (pool has no MFA today).

## Decisions

1. **Nuxt 4 static (`nuxi generate`) → S3 + CloudFront.** The app is a pure SPA behind a login; SSR would buy nothing and cost a runtime. CloudFront sits in front for TLS, caching, and a stable origin for Cognito callbacks. Alternative considered: Amplify Hosting — rejected to keep all infrastructure in this repo's CloudFormation/SAM discipline.

2. **Custom login via Cognito SRP, Amplify Auth JS for the protocol.** Hand-rolling SRP is unnecessary risk; Amplify's `signIn`/`fetchAuthSession` handles SRP, refresh, and challenge flows. Tokens live in **memory + `sessionStorage`** (per-tab persistence, cleared on tab close) rather than localStorage — acceptable XSS tradeoff for an internal tool, and it keeps tokens out of other tabs. Alternative considered: hosted UI — rejected (product decision: the login page is the design system's first showcase).

3. **Tokens as Tailwind v4 `@theme` CSS variables; dark is `:root`.** One token file (`web/app/assets/css/tokens.css`) defines the dark palette as the default root and a light palette under `[data-theme="light"]` for later; components consume semantic tokens (`bg-surface`, `text-muted`, `border-subtle`, `status-ready`…), never raw palette colors. This makes "dark-from-day-one" structural rather than a toggle.

4. **Docs are the design system's home.** `docs/development/design-system.md` documents tokens, primitives, and patterns (user decision: docs over Histoire/Storybook). To fight rot, every documented token/primitive must map to real code, and the doc lists the canonical component paths; a short "component gallery" route (`/design-system`, admin-only, excluded from prod nav) renders the primitives so the doc can reference living examples without maintaining a second toolchain.

5. **One typed API client layer, no state library.** A `useApi` composable wraps `$fetch` with the access token, one 401→refresh→retry, and typed response models mirroring the OpenAPI contracts. Server state stays in `useAsyncData`; a Pinia store holds only session/identity. Alternative considered: full OpenAPI codegen — deferred; the surface is small and hand-written types stay honest with less machinery.

6. **Polling, not websockets** (user decision): subscribe and other 202 flows poll the affected resource on a short interval (2s, max ~10s) then settle to a manual-refresh state. Failures surface the machine reason (`NOT_ENTITLED`, etc.) verbatim.

7. **Route guards mirror, never replace, API authorization.** `web/app/middleware/` gates `/admin/*` to platform groups and owner routes to users with team groups, but the backend remains the only real boundary — the guard is navigation hygiene, not security.

### Route map

```
/login                        custom SRP login (design system showcase)
/                             redirect by persona
/admin                        platform: pending approvals inbox (inbox-first)
/admin/domains                platform: all domains, enable/disable
/domains                      owner: my domains (domain-first dashboard)
/domains/new                  owner: request form (owning-team picker)
/domains/[name]               owner: detail — overview · schemas (producer scope)
                              · subscriptions outbound · access grants · edit
/catalog                      discover: entitlement-scoped catalog
/catalog/[producer]/[schema]  schema detail + subscribe confirm (202 + polling)
/subscriptions                my subscriptions (inbound) + outbound per domain
/design-system                component gallery (dev/admin)
```

## Risks / Trade-offs

- [Token theft via XSS with browser-held tokens] → sessionStorage (not localStorage), strict CSP on the CloudFront response headers, no third-party scripts; refresh tokens bounded by pool validity.
- [Design-system doc rots without tooling enforcement] → doc mandates code-referenced tokens; gallery route exercises the real components; doc review is part of UI PR checklist.
- [Static hosting config drift (API URL baked at build)] → runtime config: a `/config.json` served from the bucket (written by the deploy target from stack outputs) read at boot; rebuild not required on stack changes.
- [Auth-gated SPA on CloudFront: deep links 404] → distribution maps 403/404 to `index.html` (SPA fallback) and the router re-checks auth client-side.

## Migration Plan

Additive: new stack resources (bucket, distribution), new client auth flows (change 1), new callback URLs. Deploy backend (change 1) first, then `make deploy-web`. Rollback: remove the web stack resources; no backend coupling.

## Open Questions

- Light theme is tokenized but unpolished — first dark-only ship is acceptable; light polish when a user asks.
- Exact font choice (system stack vs one variable font) decided during the design-system task; both are compatible with the token structure.
