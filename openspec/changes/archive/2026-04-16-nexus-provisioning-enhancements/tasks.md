## 1. Naming Convention Flip

- [x] 1.1 Update `NamespaceResolver::default_pattern()` from `{env}-{org}-{domain}` to `{org}-{env}-{domain}` in `lib/control-plane-common/src/namespace.rs` and fix all tests
- [x] 1.2 Update SAM template `NamespacePattern` parameter default and all `${Environment}-${Organization}` resource name patterns to `${Organization}-${Environment}` in `template.yaml`
- [x] 1.3 Update trust-stack role naming from `${Environment}-${Organization}-cp-trust-role` to `${Organization}-${Environment}-nexus-trust-role` and all IAM resource scoping patterns in `trust-stack.yaml`
- [x] 1.4 Update provisioning workflow resource name constructions from `'${Environment}' & '-' & '${Organization}'` to `'${Organization}' & '-' & '${Environment}'` in `provisioning.asl.yaml` (will be done in full workflow rewrite)
- [x] 1.5 Update approval workflow event source and any name references in `approval.asl.yaml`

## 2. Rebrand to EventBridge Nexus

- [x] 2.1 Rename event source prefix from `controlplane.domain-management` to `nexus.domain-management` in all Lambda functions (`register.rs`, `send-approval-notification`, `send-rejection-notification`) and state machines
- [x] 2.2 Rename control plane event bus from `{org}-{env}-control-plane-bus` to `{org}-{env}-nexus-bus` in `template.yaml` and all references
- [x] 2.3 Rename trust role from `{org}-{env}-cp-trust-role` to `{org}-{env}-nexus-trust-role` in `trust-stack.yaml`, `template.yaml`, and `provisioning.asl.yaml`
- [x] 2.4 Update SAM template logical IDs referencing "ControlPlane" to "Nexus" (e.g., `ControlPlaneEventBus` → `NexusEventBus`, `ControlPlaneTrustAnchorRole` → `NexusTrustAnchorRole`)
- [x] 2.5 Update `README.md`, `AGENTS.md`, and OpenSpec `config.yaml` context to reflect EventBridge Nexus naming
- [x] 2.6 Update `api/openapi.yaml` title, description, and any references to "control plane"

## 3. New Account Detection

- [x] 3.1 Add `CheckNewAccount` state to provisioning workflow after `SetProvisioningStatus` — query `by-owner` GSI for `ownerAccountId` with filter for `ACTIVE` status, assign `$isNewAccount` boolean
- [x] 3.2 Add `isNewAccount` flag to the `DomainCreationStarted` event payload in the provisioning workflow
- [x] 3.3 Update `send-approval-notification` Lambda to accept and display `isNewAccount` flag in the approval email body with trust-stack deployment advisory
- [x] 3.4 Add DynamoDB `Query` permission for the `by-owner` GSI to the provisioning workflow role in `template.yaml`

## 4. Saga Rollback

- [x] 4.1 Add compensation states to `provisioning.asl.yaml`: `CompensateIntegrationDlq`, `CompensateAppEventBus`, `CompensateDlqDepthAlarm`, `CompensateFailedInvocAlarm`, `CompensateDashboard`, `CompensateSubscriptionDlq`, `CompensateSchemaRegistry`, `CompensatePlatformEventBus` — each with Choice guard checking if the corresponding `$assign` variable is set
- [x] 4.2 Add `DeleteEventBus`, `DeleteQueue`, `DeleteAlarms`, `DeleteDashboards`, `DeleteRegistry` permissions to the provisioning workflow role in `template.yaml` (platform account scope)
- [x] 4.3 Wire all resource creation `Catch` blocks to route to `StartCompensation` state instead of directly to `EmitCreationFailed`, passing the error context and the set of created resource variables
- [x] 4.4 Add `EmitCreationFailed` and `UpdateDomainFailed` states at the end of the compensation chain (after all cleanup) with enriched error detail including compensation outcomes
- [x] 4.5 Verify trust-stack.yaml already includes `events:DeleteEventBus` and `sqs:DeleteQueue` permissions for app-account cleanup (already present — confirm)

## 5. Namespace Parameters and RAM Sharing

- [x] 5.1 Add `CreateNamespaceParameter` state to provisioning workflow after `CreateDlqDepthAlarm` — `ssm:PutParameter` at `/namespace/{domainName}` with value `{org}-{domainName}`, type `String`, tier `Advanced`
- [x] 5.2 Add `EnsureRAMResourceShare` state — `ram:GetResourceShares` to check for `{org}-domain-namespaces`, create if not exists with AWS Organization as principal
- [x] 5.3 Add `AssociateParameterToShare` state — `ram:AssociateResourceShare` to add the SSM parameter ARN to the share
- [x] 5.4 Add SSM and RAM IAM permissions to provisioning workflow role in `template.yaml`
- [x] 5.5 Add `NamespaceResolver::namespace()` method to return `{org}-{domain}` (environment-independent) in `namespace.rs` with tests
- [x] 5.6 Include namespace parameter path in `UpdateDomainRecord` resource ARNs and `DomainCreationSucceeded` event payload

## 6. Verification and Documentation

- [x] 6.1 Run `cargo test --workspace` and fix any failures from naming convention and rebrand changes
- [x] 6.2 Run `sam validate` to verify SAM template correctness after all changes (pre-existing circular dependency warnings from task-token pattern — not caused by this change)
- [x] 6.3 Update decision log at `docs/development/decisions.md` with: naming convention flip rationale, rebrand decision, saga pattern choice, RAM sharing approach, and domain namespace definition
