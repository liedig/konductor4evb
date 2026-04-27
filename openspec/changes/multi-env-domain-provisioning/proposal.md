## Why

A domain represents the events owned by a team — Sales, Billing, Orders. In practice, a team operates that domain across several environments: `dev` for experimentation, `staging` for integration testing, `prod` for production traffic. Each environment needs its own isolated set of resources (buses, DLQs, registries, dashboards, alarms) so that a dev-side schema change or misrouted subscription can't leak into production.

Today, Nexus provisions exactly **one** set of domain resources per approval, pinned to whatever environment the Nexus platform happens to have been deployed into. The provisioning state machine hard-codes `${Environment}` as a SAM deploy-time substitution, so every step builds names like `acme-${Environment}-sales-domain-bus` from a single value. There is no way for a domain requester to ask for `[dev, staging, prod]`, and no way for the workflow to fan out.

This is a missing capability, not a bug. This change adds the feature that should have been there all along (domains are environment-aware). A sibling change, `nexus-platform-drop-environment`, follows this one and removes the now-unused `Environment` parameter from the Nexus platform templates; that cleanup is blocked until this change has moved the state machine to per-domain env sourcing.

## What Changes

### API and data model

- `RegisterDomainRequest` is **unchanged** at the API surface — no `environments` field. Assumption: **one domain → one owner account**, all envs in the same application account.
- The set of environments is **platform-configured**, not per-request. Nexus core stack gains an `Environments` SAM parameter (type `CommaDelimitedList`, default `dev,staging,prod`; each entry matches `^[a-z0-9][a-z0-9-]{0,31}$`). The command handler reads this config at cold start and stamps the list onto every new domain's record on registration.
- Domain record gains a first-class `environments` field (snapshotted at registration time — later platform config changes do not retroactively alter existing domains).
- Domain record's `resourceArns` becomes a **map keyed by environment**: `{ dev: {...}, staging: {...}, prod: {...} }`. Each inner record contains the per-env resource ARNs today's schema holds flat.
- `DELETE /domains/{id}` must tear down all per-env resource sets.
- Adding or removing environments on an existing domain is **out of scope for v1** (would require partial-provisioning/partial-teardown semantics this change does not design).

### Trust role rename (pulled forward from `nexus-platform-drop-environment`)

To provision multiple envs of a domain into a single application account, the trust role in that account must not itself be env-scoped — one role serves `dev`, `staging`, and `prod`. This change renames the trust role from `${Organization}-${Environment}-nexus-trust-role` to `${Organization}-nexus-trust-role` as a prerequisite step. The rest of the platform renaming (platform event bus, DynamoDB table, API Gateway, etc.) stays in `nexus-platform-drop-environment`.

### Workflow restructure — embedded state machines

Split the current monolithic `provisioning.asl.yaml` into three:

- **`provision-domain.asl.yaml`** (new, outer coordinator): domain-level steps (namespace parameter, RAM share), then a parallel Map over `environments` starting `provision-env` executions, then success/failure handling including teardown of successful envs if any env failed.
- **`provision-env.asl.yaml`** (extract existing happy-path + its compensation): everything the current state machine does for one environment — create platform bus, set PutEvent policy, create schema registry, create subscription DLQ, assume role, create app bus, create integration DLQ, create dashboard, create alarms — plus its existing per-env saga compensation for partial-failure within that env.
- **`deprovision-env.asl.yaml`** (new, derived from the compensation chain): idempotent teardown of one env's resource set. Reused by both (a) outer rollback when one env failed but others succeeded, and (b) the `DELETE /domains/{id}` flow (wiring that up is out of scope here, but the unit exists).

### Concurrency and failure semantics

- Per-env provisioning runs **in parallel** (`Type: Map`, `MaxConcurrency: 0` = unbounded). Resource names are env-scoped so there's no shared-resource contention.
- **All-or-nothing contract**: if any env fails, the domain is marked `PROVISIONING_FAILED`, and the outer workflow triggers `deprovision-env` for every env that succeeded. The domain owner never sees a partially-provisioned domain.
- Inner `provision-env` failures first run their own internal saga (cleaning up whatever they started to create before failing) — unchanged from today's behavior — and then the outer workflow walks the already-succeeded siblings and tears them down too.
- **Namespace parameter and RAM share are never rolled back.** Once a domain has been approved, its identity (`{org}-{domain}` in SSM) persists so that a retry reuses it.

### Notifications

- Domain owners receive **binary** notifications: succeeded, or "provisioning encountered a problem, platform team has been notified." No per-env detail is leaked.
- Platform team receives the full failure detail via the existing `DomainProvisioningFailed` event on `{org}-nexus-bus`, extended to carry `failedEnvironment` alongside the existing `failedStep`, `errorCode`, `errorMessage`, and `compensationErrors`. Routing that event to a platform ops channel (SNS/Slack) is tracked separately.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `domain-management`: Nexus gains a platform-configured environment list (SAM parameter). The domain record schema gains a top-level `environments` field (snapshotted at registration from platform config) and changes `resourceArns` to a map-by-env structure. The `RegisterDomainRequest` API surface is unchanged. The `DomainProvisioningFailed` event gains `failedEnvironment`.
- `domain-provisioning-workflow`: single state machine becomes three (`provision-domain`, `provision-env`, `deprovision-env`). Parallel Map over environments. All-or-nothing contract across envs with outer teardown of successful siblings on partial failure. Namespace/RAM operations stay at the domain level, above the Map, and are never rolled back.
- `resource-topology`: existing per-env naming and isolation requirements are preserved and reinforced — this change makes the capability first-class rather than implicit.
- `cross-account-trust-model`: Trust role is renamed to `{org}-nexus-trust-role` and becomes env-independent; the same role serves all environments of all domains in an application account. Trust stack no longer accepts an `Environment` parameter. Resource-scoping patterns in the role's inline IAM policies remain `{org}-{env}-*` so the single role can manage per-env domain resources.

## Impact

- **API change**: `RegisterDomainRequest` is **unchanged** (environments are platform-configured, not request-supplied). `DomainDetail.resourceArns` shape changes from a flat record to a map — breaking for any existing consumer. Acceptable pre-production. `DomainDetail` and `DomainSummary` gain an `environments` field (read-only, sourced from the domain record).
- **Data model migration**: existing domain records (if any) would have `resourceArns` restructured. Pre-production — no migration implemented.
- **State machine rewrite**: `provisioning.asl.yaml` is replaced with three new files. SAM template wires up one outer state machine; inner machines invoked via `StartExecution.sync:2`.
- **Trust role rename**: `trust-stack.yaml` is updated to name the role `${Organization}-nexus-trust-role` and drop the `Environment` parameter. This is the only platform-side rename included in this change; all other platform renames stay in `nexus-platform-drop-environment`.
- **Ordering**: this change lands **before** `nexus-platform-drop-environment`. The platform still carries an `Environment=dev` SAM parameter during and after this change — it controls platform resource names (e.g., `{org}-dev-nexus-bus`) only, not provisioning-workflow substitutions. `nexus-platform-drop-environment` removes that residual parameter afterwards.
- **Out of scope**: per-env status tracking within `PROVISIONING_FAILED`, mutating environments on an existing domain, org-level env allowlists, platform-ops-channel alerting.
