# Operational Exercises — DynamoDB grant-engine spike

> Spike artifact (tasks 6.1-6.3). Qualitative exercises grounded in the spike adapter
> (`src/dynamo.rs`, `src/in_memory.rs`), the representative grant model (`design.md`), and the
> frozen scenario suite. Findings feed the readiness rubric and the readiness decision.

## 6.1 Audit reconstruction and local-testability

### Audit reconstruction

- **Grant-table audit items**: the representative model stores `AuditRecord` items in the grant
  table. The in-memory adapter pushes an audit record on every `revoke()`; the DynamoDB adapter
  flips grant/subscription `state` but does **not yet write a dedicated audit item** (recorded as a
  production gap). For production, every grant mutation (create, revoke, entitlement change, schema
  registration, subscription disablement) MUST write an audit item keyed for reconstruction.
- **Reconstruction access path**: the `grant_id-index` GSI already indexes every grant-scoped item
  by `grant_id`, so an audit trail for a specific grant is a single GSI Query. Time-ordered
  reconstruction needs a second admin GSI (`GSI1`, e.g. `pk=AUDIT#YYYY-MM`, `sk=timestamp#grant_id`)
  declared in the representative model but not provisioned in the spike sandbox.
- **CloudTrail correlation**: DynamoDB API calls (PutItem/UpdateItem/Query) are captured by
  CloudTrail data-plane logging, providing an independent, tamper-evident reconstruction source
  that does not depend on application-written audit items. Recommendation: rely on CloudTrail for
  API-level audit and write application audit items only for semantic events (revoke, entitlement
  change) that CloudTrail cannot reconstruct (e.g. "why" a subscription was disabled).
- **Finding**: audit reconstruction is **supported** by the model (GSI by grant_id + CloudTrail).
  Production must (a) write semantic audit items on mutations and (b) provision the time-ordered
  admin GSI. Not a blocker; an implementation detail.

### Local-testability

- **In-memory adapter parity**: `InMemoryGrantAdapter` mirrors the grant-table key model and access
  patterns (key-condition lookups, no scans for decisions) and passes the entire frozen suite
  locally with no AWS credentials. This lets the production authorization logic be developed and
  tested fully locally before any cloud cost.
- **Shared runner**: `ScenarioRunner` is generic over `GrantAdapter`, so the same frozen suite
  validates both the in-memory and DynamoDB adapters — the cloud run is a re-verification, not a
  separate test design. This is a significant testability strength.
- **Frozen fixtures**: the manifest is hash-frozen, so local and cloud runs are guaranteed to
  evaluate identical scenarios. Evidence references the hash for traceability.
- **Finding**: local-testability is a **strength** of the DynamoDB design. No blocker.

## 6.2 Policy administration and operational complexity

### Policy administration

- **Grant lifecycle as items**: role bindings, entitlements, and schema registrations are individual
  items mutated by PutItem/UpdateItem. Administration is ordinary DynamoDB item management — no
  separate policy store, DSL, or engine to learn. This is simpler than a dedicated policy engine
  (e.g. AVP) and uses skills the team already has.
- **Concurrent changes**: the in-memory adapter serializes via a `Mutex`; the DynamoDB adapter
  currently does unconditional writes. Production grant mutation MUST use conditional writes
  (`ConditionExpression` asserting expected `state`/`version`) or `TransactWriteItems` to prevent
  lost updates when two operators mutate the same grant. The spike does not exercise this; it is a
  required production control (recorded as a residual risk in the SLO recommendations).
- **Quotas**: DynamoDB imposes no per-table item quotas and on-demand capacity auto-scales; the
  relevant limits (table/GSI count per account, item size 400 KB) are nowhere near exceeded by a
  grant table. No quota concern.

### Operational complexity

- **Operator steps** are small and enumerable: register schema, bind publisher/subscriber, create
  entitlement, revoke grant (auto-reconciles subscriptions), disable subscription. Each is one
  DynamoDB write (plus synchronous reconcile for revoke).
- **Failure modes**:
  - *Partial revoke*: the DynamoDB adapter flips the grant then reconciles subscriptions in the same
    `revoke()` call. If the process fails between the flip and reconciliation, a subscription could
    remain active without coverage. Production must make revoke idempotent (re-reconciliation on
    retry) or transactional. The spike's synchronous-within-`revoke()` design minimizes this window.
  - *Throttling*: on-demand capacity avoids provisioned throttles; CloudWatch alarms on
    `ThrottledRequests` and `UserErrors` are the recommended guardrails.
  - *Backup/recovery*: PITR or on-demand backups on the grant + subscription tables; restore
    repopulates grants and subscriptions atomically (single-table restore per table).
- **Alarms** (recommended, minimal set): `ThrottledRequests > 0`, `UserErrors` rate, a custom alarm
  for "active subscription with no covering entitlement" (a reconciled-state invariant, polled
  periodically), and the RRU spend guardrail from the cost worksheet.
- **Finding**: operational complexity is **low**. The main production control to add is conditional
  writes / transactions for concurrent grant mutation and idempotent revoke reconciliation. Not a
  blocker; standard DynamoDB practice.

## 6.3 AWS account principal, Cognito integration, and hierarchy/inheritance

### AWS account principal

- The model authenticates AWS-account principals by **account id** (`PRINCIPAL#aws-account#<acct>`).
  The assumed-role/user ARN is captured for audit (`AuditProvenance::IamPrincipalArn`) but is
  **not** consulted for the decision — verified by S11 (same decision for the cognito-group and
  aws-account equivalents; assumed-role ARN yields the same decision; forged issuer denies).
- **Cross-account verification (available this session)**: the demo profile (domain account
  `210987654321`, assumed-role `Admin/operator-session`) provides a real IAM principal whose caller
  identity ARN is `arn:aws:sts::210987654321:assumed-role/Admin/operator-session`. This can be used
  to confirm, against the cloud adapter, that a real STS-issued principal resolves to the
  account-id key and receives the decision bound to `444455556666`-style fixtures (after mapping the
  real account id into a fixture grant). This is an optional parity confirmation; the frozen S11
  already proves the logic in-cloud with the fixture account id.

### Cognito integration

- Cognito-group principals are authenticated by the **trusted issuer** URL. The adapter checks
  `subject.issuer == trusted_issuer` and denies (`untrusted-issuer`) on mismatch — verified by S11
  forged-issuer. The frozen manifest's issuer is a placeholder string; production wires the real
  Cognito user-pool issuer. No Cognito API call is needed for the decision (the authorizer supplies
  the verified group + issuer), so there is no Cognito latency or availability dependency on the
  decision path.
- Enterprise AD integration is via Cognito federation (the user-pool identity provider), not the
  grant engine; the grant engine sees only Cognito groups. This keeps the engine decoupled from the
  IdP.

### Hierarchy / inheritance

- The model uses **explicit bindings and entitlements with no implicit role inheritance**. A
  subscriber is authorized for a schema only by an active binding to the consumer domain AND an
  active entitlement covering that producer/schema. Producer-domain entitlements cover all current
  and future schemas of a producer (dynamic growth, S08); exact-schema entitlements cover exactly
  one schema and do not expand (S08). There is no "admin implies editor implies viewer" ladder.
- **Finding**: the absence of implicit inheritance is a deliberate simplification that makes
  authorization outcomes predictable and audit-friendly (every allow is traceable to an explicit
  grant). It shifts expressive power to the grant model (which roles/domains/schemas to bind) rather
  than a permission hierarchy. This is adequate for Konductor's domain-pairing authorization needs and
  avoids the surprise-access and combinatorial-review problems of hierarchical RBAC. If a future
  requirement needs hierarchy, it would be modeled as explicit grants at each level (no engine
  change) or trigger the AVP reevaluation (see `design.md`).

## Summary

| Exercise | Outcome | Blocker? |
|---|---|---|
| 6.1 Audit reconstruction | supported (GSI by grant_id + CloudTrail); write semantic audit items + time-ordered GSI in production | no |
| 6.1 Local-testability | strength — in-memory parity, shared runner, frozen fixtures | no |
| 6.2 Policy administration | simple item-based; add conditional writes/transactions for concurrent mutation | no |
| 6.2 Operational complexity | low; idempotent revoke reconciliation + alarm set in production | no |
| 6.3 AWS account principal | account-id keyed, ARN audit-only, S11 verified; optional real-principal parity via demo profile | no |
| 6.3 Cognito integration | trusted-issuer check, no decision-path IdP dependency | no |
| 6.3 Hierarchy/inheritance | explicit grants, no implicit inheritance — predictable and auditable | no |

No operational exercise uncovered a production blocker. The production controls to add (conditional
writes, idempotent revoke, semantic audit items, alarm set) are standard DynamoDB implementation
details, not design flaws.
