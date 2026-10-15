# Proposal

## Why

Konductor has no promotion operation. "Promoting" a schema means pasting its content again into another stage's registry, and the console cannot do even that, because it is fixed to the `dev` stage. Each registry assigns its own version numbers, so after a re-paste the same number can mean different content in different environments. For example, staging v2 might contain dev v3. Event stamping, exact-version subscription pins, and the catalog all treat the number as one contract everywhere, so this mismatch goes unnoticed. Separately, any append to an environment with live subscriptions stops delivery there: the pins stay on the old version while publishing stamps the new one.

## What Changes

- **BREAKING**: Schema creation and version append are accepted only on the first configured environment (`environments[0]`). Every other environment receives versions only through promotion.
- New `POST /{target}/schemas/promotions` on both API surfaces, taking `{domain, eventName}`. The stage names the target environment, and the source is the environment immediately before it in the configured list. Promotion is strictly linear, and the order of the environment list is the promotion order.
- Promotion replays the source's missing versions into the target registry in order, so version numbers and content match across environments. Before writing, a content-prefix check confirms that the target's versions 1..k are byte-identical to the source's. If they are not, promotion fails closed (`DIVERGED`) and writes nothing. Replayed steps are trusted because their content matches the source exactly, so the compatibility gate is not run again.
- Promotion is asynchronous (`202`) and runs as a durable workflow with a deterministic execution name. Only one promotion may run per (schema, target environment); a concurrent request returns `409 Conflict`. A promotion record stores the phase, the actor, and the outcome.
- Promotion re-pins every materialized target subscription to the last replayed version. Rules first widen to match the old version, the in-between versions, and the new version, then narrow to the new version after a short drain window, so delivery continues throughout. If replay stops partway, rules narrow to the last version actually written. Appends in the first environment re-pin that environment's subscriptions the same way.
- `SchemaVersionPublished` is emitted once per promotion, after the last replayed version, so promotion-following extension never pins an in-between version. New producer-facing lifecycle events: `SchemaPromotionStarted`, `SchemaPromotionCompleted`, `SchemaPromotionFailed`.
- New grant-engine action `schema:promote`, allowed for the `publisher` binding on the domain.
- Console: a new producer schema page showing the environment pipeline in configured order, how many versions each environment is behind, promote actions, version history, subscribers by environment, and promotion progress. The domain Schemas panel becomes a compact list of pipeline badges that link to this page. Authoring always targets `environments[0]`, and the console can call any configured environment's stage.

## Capabilities

### New Capabilities

None. Promotion extends the existing schema-management bounded context.

### Modified Capabilities

- `schema-management`: authoring restricted to the first environment; a new promotion requirement covering replay, the content-prefix check, linear order, concurrency, `schema:promote` authorization, asynchronous status, and producer lifecycle events; `SchemaVersionPublished` emitted once per promotion.
- `event-subscriptions`: pins move to the latest version when a promotion runs or the first environment receives an append, using widen, drain, and narrow; a partial replay pins to the last written version. This replaces the rule that appends to existing environments never re-pin.
- `system-architecture`: the order of the domain environment list is defined as the promotion order.
- `web-ui`: the producer schema page; pipeline-ordered environment display; authoring targets the first environment; the console can address environment stages.

## Impact

- Lambdas: `schema-admin` (first-environment guard, emission changes), a new promotion handler and status read, `materialize-subscription-env` reused for re-pinning, `subscription-extender` (single end-of-promotion signal).
- Infrastructure: a Step Functions promotion workflow with a drain Wait state; promotion records in DynamoDB whose stream starts the workflow (to be classified against the side-effects rubric); generating CloudFront stage routes from `environments` instead of hardcoding `/dev`, `/staging`, and `/prod`; `config.json` authoring stage taken from `environments[0]`.
- API: OpenAPI contracts for the promotion endpoint and status read on the OAuth and IAM surfaces. Direct writes to stages other than the first now return an error.
- Web: a new route under `/domains/{name}/schemas/`, `useApi()` addressing each environment's stage, and Playwright coverage for promotion flows.
- Docs: a decision record superseding the parked `gate-schema-promotion` deferral, side-effects records, and a runbook note that a publish DLQ redrive after narrowing drops events stamped with the old version.
- Assumes no schemas exist at launch, so no migration is needed.
- Spikes: whether `UpdateSchema` with content identical to the latest version creates a new version, and whether `ClientTokenId` makes replay retries idempotent.
