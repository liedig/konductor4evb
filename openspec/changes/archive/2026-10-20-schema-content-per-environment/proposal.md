# Proposal

## Why

The producer schema page shows which version each environment runs, but not the schema document itself. The only console surface that renders content is the consumer catalog page, which requires an entitlement and answers a consumer's question (the composed contract), not a producer's ("what exactly is running in staging?"). The describe API is no help either: it returns the globally highest version's content, not environment-scoped content. Today, verifying what v3 looks like in staging means leaving the console for `aws schemas describe-schema` against the environment's registry.

## What Changes

- New read-only endpoint on both API surfaces: `GET /schemas/{schemaName}/content?domain={domain}&environment={env}[&version={n}]`, returning the registry content of one version in one environment. `version` defaults to the environment's latest. Same authorization rule as the existing schema detail route: a `publisher` binding on the producer domain, or a platform role.
- The producer schema page makes versions inspectable: each pipeline node holding a version offers a view affordance that opens a modal with the content, pretty-printed and copyable. Version history rows offer the same per environment a version has reached.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `schema-management`: adds a per-environment content read operation to the schema API.
- `web-ui`: adds content inspection to the producer schema page.

## Impact

- `event-streams/functions/describe-schema`: new route handler branch (the function already serves `/schemas/{schemaName}` and `/schemas/{schemaName}/contract`).
- `api/openapi-iam.yaml`, `api/openapi-oauth.yaml`: new route on both surfaces.
- `web/app/pages/domains/[name]/schemas/[schema].vue` plus a viewer component using `NxModal`, existing pretty-print and `useCopyToClipboard()` conventions from the catalog page.
- No schema of record changes, no new infrastructure, no new permissions: the describe-schema role already reads every environment's registry.
