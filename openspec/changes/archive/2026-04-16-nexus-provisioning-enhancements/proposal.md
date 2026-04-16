## Why

The project has outgrown its original "control plane" framing. It is not a control plane in the control/data plane sense — it is a central integration platform for EventBridge. The name "EventBridge Nexus" better communicates what it does: the connection point that links domains, accounts, and event flows.

Beyond naming, the provisioning workflow has gaps that need addressing before building further capabilities. There is no rollback — if provisioning fails partway through, orphaned resources are left behind requiring manual cleanup. The control plane has no awareness of which accounts have been onboarded — it discovers missing trust roles only when `AssumeRole` fails during provisioning. Domain namespaces are invisible to application accounts, forcing teams to look up naming conventions or make cross-account calls to discover what domains exist. Finally, the resource naming convention `{env}-{org}-{domain}` groups by environment rather than by domain, making it harder to reason about resources across environments.

## What Changes

### Rebrand: EventBridge Control Plane to EventBridge Nexus

- Rename across all documentation, code comments, event source prefixes, resource names, and config
- Event source prefix changes from `controlplane.domain-management` to `nexus.domain-management`
- OpenSpec config context updated to reflect new name and purpose

### Naming convention: `{org}-{env}-{domain}` replaces `{env}-{org}-{domain}`

- **BREAKING**: All resource names flip from `{env}-{org}-{domain}` to `{org}-{env}-{domain}` (e.g., `dev-acme-billing-domain-bus` becomes `acme-dev-billing-domain-bus`)
- Domain namespace is `{org}-{domain}` (environment-independent, e.g., `acme-billing`)
- Trust role naming follows the same flip: `{org}-{env}-cp-trust-role`
- DynamoDB table, queues, event buses, dashboards, alarms — all updated

### New account detection in provisioning workflow

- Before `AssumeRole`, the provisioning workflow queries the `by-owner` GSI for the `ownerAccountId`
- If no other domains exist for that account (excluding the current request), the account is flagged as new
- New account status is surfaced in the approval notification so the approver knows trust-stack deployment is required
- Provisioning workflow attempts `AssumeRole` — if it fails for a new account, the error context makes it clear the trust-stack has not been deployed

### Saga rollback on provisioning failure

- Replace the current catch-and-fail pattern with compensating transactions
- Each resource creation step tracks its output in assigned variables (already done via `Assign`)
- On failure at any step, a compensation chain walks backwards through created resources:
  - Integration DLQ (app account) → delete if created
  - App event bus (app account) → delete if created
  - DLQ depth alarm (platform) → delete if created
  - Failed invocations alarm (platform) → delete if created
  - Dashboard (platform) → delete if created
  - Subscription DLQ (platform) → delete if created
  - Schema registry (platform) → delete if created
  - Platform event bus (platform) → delete if created
- After compensation, domain record is set to `PROVISIONING_FAILED` with error detail and `DomainCreationFailed` event is emitted

### RAM-shared namespace parameters

- When a domain is provisioned, an SSM Advanced Parameter is created: `/namespace/{domain}` with value `{org}-{domain}`
- A RAM resource share (`{org}-domain-namespaces`) is created if it does not exist, with the AWS Organization as principal
- The SSM parameter is associated with the RAM share
- Application accounts see `/namespace/*` parameters locally — no cross-account IAM required
- Parameters are read-only to consuming accounts; only the Nexus (platform account) can write them
- Namespace parameters are environment-independent — created once per domain regardless of which environment triggers first provisioning

## Capabilities

### New Capabilities

_(none — these are enhancements to existing capabilities and cross-cutting concerns)_

### Modified Capabilities

- `domain-provisioning-workflow`: Saga rollback, new account detection, namespace parameter creation, RAM sharing, updated resource naming convention
- `cross-account-trust-model`: Trust role naming convention flip, new account detection awareness
- `resource-topology`: Resource naming convention flip from `{env}-{org}-{domain}` to `{org}-{env}-{domain}`, domain namespace defined as `{org}-{domain}` (environment-independent)
- `system-architecture`: Rebrand to EventBridge Nexus, event source prefix change, SSM Parameter Store and RAM added as service dependencies

## Impact

### Code changes

- **Modify**: All Lambda functions and shared library references to the naming convention (string construction using `{env}-{org}` flips to `{org}-{env}`)
- **Modify**: Event source prefix in all EventBridge `PutEvents` calls (`controlplane.*` to `nexus.*`)
- **Modify**: Provisioning workflow state machine — add account detection step, saga compensation chain, SSM parameter creation, RAM share management
- **Modify**: Approval notification Lambda — surface new account flag in notification email

### Infrastructure

- SAM template: Update all resource name patterns, add SSM and RAM IAM permissions to provisioning workflow role, update event source patterns in EventBridge rules
- Trust stack: Flip role naming to `{org}-{env}-cp-trust-role`, update resource ARN scoping patterns
- State machines: Major rework of `provisioning.asl.yaml` (saga, account detection, SSM/RAM steps)

### Dependencies

- New: AWS Systems Manager Parameter Store (Advanced tier parameters)
- New: AWS Resource Access Manager (resource shares, requires AWS Organizations)
- Existing: AWS Organizations (already implicit in cross-account model, now explicit dependency for RAM)

### Migration

- **BREAKING**: Existing deployed resources use old naming convention. A migration path is needed for any live environments — either tear down and re-provision, or handle both conventions during transition. This should be addressed in the design artifact.
