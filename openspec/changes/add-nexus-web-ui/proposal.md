# Proposal: add-konductor-web-ui

## Why

Konductor is operable only through raw API calls and shell scripts; the domain request/approval, catalog discovery, subscription, and access-grant journeys have no human interface. A single role-gated web console — backed by the OAuth surface and Cognito group separation that already exist — makes the platform usable by its two personas: platform admins (approve, oversee) and domain teams (request, own, publish, subscribe).

## What Changes

- New Nuxt 4 + Tailwind CSS v4 application in `web/` inside this monorepo, static-generated and hosted behind S3 + CloudFront with the Cognito callback URLs wired through the existing `OAuthCallbackUrls` stack parameter.
- Custom, Konductor-branded login screen using Cognito SRP (enabled by enable-oauth-domain-management); the hosted UI is not used. Session handling with refresh and route guards.
- Role-gated navigation and persona dashboards: platform admins land inbox-first (pending approvals), domain owners land domain-first (their domains and status).
- Domain journeys: request form (owning-team picker from the caller's groups, owner account, emails, description, tags), my domains list, domain detail with owner-editable description/emails/tags, platform-only enable/disable.
- Approvals inbox for platform admins backed by the new pending-registrations endpoint: review request details, approve, or reject with justification.
- Catalog journeys: entitlement-scoped browse with producer/environment filters, schema detail with version-per-environment and JSON content view; producers see their own domain's schemas.
- Subscription journeys: subscribe confirmation (async 202 with short polling until ENABLED/FAILED), my subscriptions (inbound/outbound) with pause/resume/unsubscribe, failure-reason surfacing.
- Access grants on the producer domain page: list/create/delete entitlements with a names-only directory picker for the consumer domain and a schema-pattern picker with a wildcard escape hatch.
- A documented visual design system (dark-from-day-one) captured in `docs/development/design-system.md`, with tokens implemented as Tailwind v4 `@theme` CSS variables.
- Tooling: `make seed-users` personas (from enable-oauth-domain-management) are the canonical test logins; `make deploy-web`/`make web-dev` workflows.

## Capabilities

### New Capabilities

- `web-ui`: the Konductor web console — authentication/session, role-gated navigation and dashboards, the domain/catalog/subscription/access-grant screens, the visual design system, and its hosting/deployment shape.

### Modified Capabilities

(none — backend contract changes are captured by enable-oauth-domain-management)

## Impact

- **New** `web/` Nuxt app (package.json workspace separate from the Rust workspace), `docs/development/design-system.md`.
- **Stack** (`konductor-core-stack.yaml` or a small sibling stack): S3 bucket + CloudFront distribution + outputs for the web origin; `OAuthCallbackUrls` gains the web origins (dev `http://localhost:3000/callback`, prod distribution URL).
- **Depends on** enable-oauth-domain-management for: SRP client flows, owner-role authorization, scoped reads + directory projection, pending-registrations endpoint, producer catalog scope, and `seed-users.sh`.
- **Docs**: README gains a web-console section; oauth guide references the UI login.
- **CI**: build/lint for the web app added to the pipeline.
