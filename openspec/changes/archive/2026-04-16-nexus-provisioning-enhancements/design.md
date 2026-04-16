## Context

The provisioning workflow (`provisioning.asl.yaml`) creates domain resources across the platform and application accounts using Step Functions SDK integrations. It currently has no rollback capability, no account awareness, and no mechanism for sharing domain namespace references with application accounts. The resource naming convention `{env}-{org}-{domain}` groups by environment rather than domain, which makes cross-environment reasoning harder. The project name "EventBridge Control Plane" misrepresents the system's purpose.

The provisioning workflow role already has scoped permissions for EventBridge, SQS, CloudWatch, Schemas, DynamoDB, and cross-account STS in `template.yaml`. The `NamespaceResolver` in `lib/control-plane-common/src/namespace.rs` centralises resource name generation with a configurable pattern. All event sources currently use `controlplane.domain-management`.

## Goals / Non-Goals

**Goals:**

- Rename the project to EventBridge Nexus across code, config, events, and documentation
- Flip the resource naming convention to `{org}-{env}-{domain}` so resources group by domain across environments
- Detect new (previously unseen) accounts during provisioning and surface this in the approval notification
- Implement saga-pattern rollback in the provisioning workflow so failed provisioning cleans up after itself
- Create SSM Advanced Parameters for domain namespaces and share them via AWS RAM so application accounts can discover domains locally
- Define the domain namespace as `{org}-{domain}` (environment-independent)

**Non-Goals:**

- Schema registry sharing (deferred to event-routing capability)
- Automated trust-stack deployment via StackSets (account provisioning remains manual with improved detection)
- Domain deprovisioning workflow (separate future change)
- Migration tooling for existing deployed resources (addressed in migration plan below)
- Event subscription or routing capabilities

## Decisions

### Decision 1: Naming convention `{org}-{env}-{domain}`

**Choice:** Change the default namespace pattern from `{env}-{org}-{domain}` to `{org}-{env}-{domain}`.

**Rationale:** Resources should group by organisation and domain first, with environment as a secondary discriminator. When listing resources in the AWS console or CLI, `acme-dev-billing-*` and `acme-prod-billing-*` appear adjacent, making cross-environment comparison natural.

**Alternatives considered:**
- `{org}-{domain}-{env}` — Groups domain variants together but breaks the `{prefix}-*` IAM scoping pattern since the environment is a suffix. Would require more complex IAM resource patterns.
- Keep `{env}-{org}-{domain}` — Current convention; groups by environment which scatters domain resources when you have many domains.

**Impact:** The `NamespaceResolver::default_pattern()` changes to `"{org}-{env}-{domain}"`. All SAM template resource ARN patterns flip from `${Environment}-${Organization}-*` to `${Organization}-${Environment}-*`. Trust stack role naming and IAM scoping patterns follow the same flip.

### Decision 2: Domain namespace is `{org}-{domain}`, environment-independent

**Choice:** The domain namespace is defined as `{org}-{domain}` (e.g., `acme-billing`). This is a stable identity that does not change across environments. It is stored as an SSM parameter at `/namespace/{domain}` with value `{org}-{domain}`.

**Rationale:** The namespace is the domain's logical identity within the organisation. Environment is a deployment concern, not an identity concern. Application teams reference the namespace to discover and construct resource names — they should not need to know which environment they are querying.

**SSM parameter design:**
- Path: `/namespace/{domain}` (e.g., `/namespace/billing`)
- Value: `{org}-{domain}` (e.g., `acme-billing`)
- Tier: Advanced (required for RAM sharing)
- Owner: EventBridge Nexus (platform account)
- Read-only to consuming accounts

### Decision 3: RAM sharing at the AWS Organization level

**Choice:** Create a single RAM resource share named `{org}-domain-namespaces` (e.g., `acme-domain-namespaces`). Share all `/namespace/*` SSM parameters with the entire AWS Organization.

**Rationale:** Full-mesh discoverability. Every account in the organisation can see every domain namespace. This enables teams to discover available domains without cross-account IAM or API calls. Since namespaces are just identifiers (not sensitive data), broad sharing is appropriate.

**Alternatives considered:**
- Per-account RAM shares — Fine-grained access control but adds complexity for managing share membership. Not justified since namespace values are non-sensitive.
- No RAM, use Nexus API for discovery — Forces cross-account API calls and IAM setup for what should be simple parameter reads. Defeats the purpose of local references.

**Provisioning workflow integration:**
1. `CreateNamespaceParameter` — `ssm:PutParameter` (idempotent; no-op if parameter already exists with same value)
2. `EnsureRAMResourceShare` — `ram:GetResourceShares` to check existence, `ram:CreateResourceShare` if needed
3. `AssociateParameterToShare` — `ram:AssociateResourceShare` to add the SSM parameter ARN

### Decision 4: New account detection via `by-owner` GSI

**Choice:** Before `AssumeRole`, the provisioning workflow queries the DynamoDB `by-owner` GSI for `ownerAccountId`. If no other ACTIVE domains exist for that account, it is flagged as a new account.

**Rationale:** The domains table already tracks `ownerAccountId` with a GSI. No new table or entity needed — the domain table implicitly serves as the account registry. The first domain registration from any account will be flagged.

**Query design:**
- Query `by-owner` GSI with `ownerAccountId = :id`
- Filter for `status = ACTIVE` (exclude the current PROVISIONING domain and any previously failed attempts)
- If count == 0, this is a new account

**What happens for new accounts:**
- The approval notification email includes a flag: "This is a new account — trust-stack deployment required"
- The provisioning workflow proceeds normally to `AssumeRole`
- If `AssumeRole` fails (trust role not deployed), the saga rollback cleans up platform-side resources and the domain enters `PROVISIONING_FAILED` with a clear error message indicating the trust-stack is missing
- The platform team deploys the trust-stack, then re-triggers provisioning (mechanism TBD — manual re-approval or retry endpoint)

### Decision 5: Saga pattern for provisioning rollback

**Choice:** Replace the current catch-and-fail-fast pattern with a saga compensation chain. On failure at any step, the workflow walks backwards through successfully created resources and deletes them before marking the domain as `PROVISIONING_FAILED`.

**Rationale:** The current workflow leaves orphaned resources on partial failure. These require manual cleanup and are invisible to the Nexus. A saga ensures the system is either fully provisioned or fully clean.

**Implementation approach:**

The provisioning workflow already uses `Assign` to capture outputs from each resource creation step (`$platformBus`, `$schemaRegistry`, `$subscriptionDlq`, `$appBus`, `$integrationDlq`, `$dashboard`). The compensation chain uses Choice states to check which variables are set, then deletes in reverse order:

```
Compensation order (reverse of creation):
1. DeleteIntegrationDlq      (app account, if $integrationDlq set)
2. DeleteAppEventBus          (app account, if $appBus set)
3. DeleteDlqDepthAlarm        (platform, if alarm was created)
4. DeleteFailedInvocAlarm     (platform, if alarm was created)
5. DeleteDashboard            (platform, if $dashboard set)
6. DeleteSubscriptionDlq      (platform, if $subscriptionDlq set)
7. DeleteSchemaRegistry       (platform, if $schemaRegistry set)
8. DeletePlatformEventBus     (platform, if $platformBus set)
```

Each compensation step catches its own errors (best-effort cleanup). After compensation completes, the workflow transitions to `UpdateDomainFailed` and `EmitCreationFailed` as before.

App-account resource deletion reuses the assumed role credentials from `AssumeAppAccountRole`. If the failure occurred before or during `AssumeRole`, only platform-side resources need cleanup.

**Alternatives considered:**
- Manual cleanup only — Current approach. Error-prone, invisible orphaned resources.
- Parallel compensation — Delete all resources simultaneously. Risk of dependency issues (e.g., can't delete a bus with active rules). Sequential reverse-order is safer.

### Decision 6: Rebrand to EventBridge Nexus

**Choice:** Rename from "EventBridge Control Plane" to "EventBridge Nexus" across all artifacts.

**Rationale:** The system is an integration platform, not a control plane in the infrastructure control/data plane sense. "Nexus" communicates that it is the central connection point linking domains, accounts, and event flows.

**Scope of rename:**
- Event source prefix: `controlplane.domain-management` → `nexus.domain-management`
- Control plane event bus: `{org}-{env}-control-plane-bus` → `{org}-{env}-nexus-bus`
- Trust role: `{org}-{env}-cp-trust-role` → `{org}-{env}-nexus-trust-role`
- Code references: module names, log messages, error prefixes
- Documentation: README, AGENTS.md, OpenSpec config context
- SAM template: resource logical IDs referencing "ControlPlane" renamed to "Nexus"

### Decision 7: Provisioning workflow role permissions additions

The provisioning workflow IAM role needs new permissions for SSM and RAM operations:

| Service | Actions | Resource scope |
|---------|---------|----------------|
| SSM | `ssm:PutParameter`, `ssm:GetParameter`, `ssm:AddTagsToResource` | `arn:aws:ssm:*:${AccountId}:parameter/namespace/*` |
| RAM | `ram:CreateResourceShare`, `ram:GetResourceShares`, `ram:AssociateResourceShare`, `ram:ListResources` | `*` (RAM does not support resource-level permissions for share management) |

## Risks / Trade-offs

**[Breaking change] Naming convention flip affects all deployed resources** → No automated migration. For pre-production environments, tear down and re-provision. For production, a migration runbook would be needed, but production deployment is not yet in scope. The naming convention change must land before any production deployment.

**[RAM dependency on AWS Organizations] RAM sharing requires Organizations to be enabled** → This is already an implicit dependency (cross-account trust model assumes organizational structure). Making it explicit via RAM is a natural progression, not a new constraint.

**[Advanced SSM parameter cost] $0.05/parameter/month per namespace** → Negligible at expected scale (tens to hundreds of domains). Cost is justified by the discoverability benefit.

**[Saga compensation is best-effort] If a compensation step fails, resources may still be orphaned** → Each compensation step has its own error handler that logs the failure and continues to the next step. The `PROVISIONING_FAILED` error detail will include compensation failures so operators know what to clean up manually. This is a pragmatic trade-off — full transactional guarantees are not achievable across AWS services.

**[New account detection is advisory only] The workflow does not block on trust-stack deployment** → The approval notification tells the approver a new account was detected, but provisioning will still attempt `AssumeRole` and fail if the trust-stack is not deployed. This is intentional — keeping the workflow simple and letting the existing failure/retry path handle it. A future enhancement could add a wait-for-callback pattern.

## Migration Plan

1. **Pre-production only** — This change assumes no production deployment exists. All naming convention changes are clean-cut.
2. **Deploy order:**
   a. Update trust-stack.yaml with new naming convention and deploy to all application accounts
   b. Deploy updated SAM template (new naming, new permissions, saga workflow, SSM/RAM steps)
   c. Existing domain records in DynamoDB reference old resource ARNs — these domains would need to be re-provisioned or manually updated
3. **Rollback:** Revert SAM template and trust-stack to previous versions. Domain records created with new naming convention would need manual cleanup.

## Open Questions

1. **Retry mechanism for failed provisioning** — When a domain fails provisioning due to missing trust-stack, how does the platform team re-trigger provisioning after deploying the trust-stack? Options: manual re-approval, dedicated retry API endpoint, or operator CLI command. To be addressed in a follow-up change.
2. **RAM resource share cleanup** — If a domain is deprovisioned (future), should its SSM parameter be removed from the RAM share? Likely yes, but deprovisioning is out of scope.
3. **Namespace parameter idempotency across environments** — The SSM parameter `/namespace/{domain}` is created during provisioning. If the same domain exists in multiple environments, the first provisioning creates the parameter and subsequent ones are no-ops (idempotent `PutParameter` with same value). Confirm this is the intended behaviour.
