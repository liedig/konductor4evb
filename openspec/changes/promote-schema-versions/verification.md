# Verification — live end-to-end (2026-10-19, konductor-core, ap-southeast-2)

Scenario: fresh schema `PromotionSmokeEvent` (billing producer; sales + shipping
consumers), v1 created via the public API, continuous cross-env publishers
tracking the current pin, CloudWatch Logs taps on all six consumer buses
(sales in 210987654321, shipping in 777766665555).

## Results

| Assertion | Outcome |
|---|---|
| Authoring guard | v1 create + v2/v3 appends accepted only on the `dev` stage; content records written (`writtenBy: schema-admin`) |
| Backward-compat proof | constrained optional property refused (`ValidationException` naming the path); unconstrained addition accepted |
| Append re-pin | `repin-billing-PromotionSmokeEvent-dev-v2/-v3` SUCCEEDED; widen applied `[1,2,3]` before pin commit; pins moved 1→2→3 after drain |
| Promotion dev→staging | COMPLETED (`7ffe3af5`); staging pattern widened `[1,2,3,4]` during drain; narrow re-pinned 2/2 entries, `repin: {matched:2, repinned:2, failed:0}` |
| Promotion staging→prod | COMPLETED (`314485ff`); `replayedVersions: [1,2,3]`, repin 2/2 |
| Delivery continuity | published-vs-delivered per env per consumer: dev 168→177/177, staging 181→196/188, prod 54→55/55 (delivered ≥ published everywhere; excess = pre-baseline window + at-least-once). No drops across either drain |
| Content alignment | v1–v3 canonically identical across dev/staging/prod registries and the versions table |
| Aligned version numbers | `GET /schemas` reports `latestVersions: {dev:3, staging:3, prod:3}` |
| Status API | per-stage `GET /schemas/promotions` reports COMPLETED/FAILED with reason |
| Divergence | out-of-band `CreateSchema` in staging for `DivergedProbeEvent` → promotion FAILED, `reason: DIVERGED`, `failedVersion: 1` |
| Pre-existing schema | `OrderNotification` pins unchanged (dev 3, staging 1, prod AWAITING_PROMOTION); no content records required |
| Idempotent replay | staging v1/v2 already existed at promotion #3; duplicate-refusal + canonical-verify accepted them (v2 recorded under the retry's id) |

## Adjacency gate (11.3, added after the live run)

The API already derives the source as the stage's immediate predecessor, so a
skipped-environment promotion (dev straight into prod) is not expressible
through it. The gap was the workflow itself: `plan` accepted any
source/target pair, so a direct `StartExecution` with a planted lease could
skip an environment. `plan` now verifies
`promotion_source(target) == source` before its first registry read and the
workflow fails the promotion with reason `SKIPPED_ENVIRONMENT`. Covered by
unit tests (non-adjacent, authoring-as-target, unknown target — all refused
with zero AWS calls) rather than a live probe, since exercising it requires
planting a promotions-table item.

## Defects found and fixed during verification

1. **`AWS::ApiGateway::Account` missing**: explicit per-env stages with
   `AccessLogSetting` require the account-level CloudWatch role; the stack
   update rolled back without it. Added role + account resource.
2. **SFN rejects YAML aliases and `Catch` with `End`**: state machine
   definitions failed validation (`Retry: *alias`, `Catch … End: true`,
   duplicate inner Map state names). Inlined retries, `Next`-pointing catches,
   unique state names; structural regression tests added.
3. **`schemas:CreateSchema` missing from the steps role**: the v1
   first-arrival path failed replay with ForbiddenException. Policy updated.
4. **`UpdateSchema` rejects `ClientTokenId`** ("Malformed request"): removed
   from the replay write; the minted-version check + conflict-verify carry
   idempotency. (CreateSchema has no token field in the SDK model at all.)

Cleanup: subscriptions removed (REMOVED), smoke schemas deleted from all
registries, version records deleted, tap rules/log groups/policies removed in
all three accounts. Live environment restored to its pre-verification state
(OrderNotification subscriptions untouched).
