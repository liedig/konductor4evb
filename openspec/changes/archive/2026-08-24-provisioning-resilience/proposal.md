# Provisioning resilience

## Why

A domain whose provisioning saga fails lands in `PROVISIONING_FAILED` and stays there: re-registration is rejected (only absent or `REJECTED` names may re-register), and no retry action exists. Operators must delete the domain record by hand — and separately clean up its role bindings, because finalize-approval's `attribute_not_exists` binding puts would otherwise cancel the approval transaction of a re-registered domain. This actually happened with the `sales` domain: an orphaned estate from an earlier test era collided at the first provisioning step (`CreatePlatformEventBus` → `ResourceAlreadyExistsException`), and recovery was entirely manual.

Provisioning also discovers pre-existing resource name collisions only at provision time — after a platform admin has already approved. A collision detectable at registration should fail the request before it consumes review effort.

## What Changes

- **Retry failed provisioning**: a platform-admin action re-runs the provisioning workflow for a `PROVISIONING_FAILED` domain. The action conditionally resets the domain to `APPROVED` (clearing `errorDetail`) and emits a `DomainProvisioningRetryRequested` lifecycle event carrying the registration fields from the domain record; a new EventBridge rule starts the provisioning workflow on that event. Retry is idempotent-safe: the saga's own per-env compensation handles partially created resources from the failed run.
- **Registration preflight collision check**: before a registration record is written, the registrations consumer checks for colliding platform-owned resource names (per-environment event buses and the domain namespace SSM parameter). On collision the record is written as `REJECTED` with `decidedBy: system` and a justification naming the colliding resources, so the requester sees the outcome in their domains list and may re-request once the collision is cleared (re-registration over `REJECTED` is already permitted).

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `domain-management`: new requirement for the provisioning-retry action (platform-admin only, `PROVISIONING_FAILED` precondition, lifecycle event); the registration requirement gains the preflight collision-check behavior with a deterministic system rejection.

## Impact

- **API**: `POST /domains/{domainName}/state` gains a `retry-provisioning` action (OAuth surface, platform-admin).
- **State machines**: none modified; a new EventBridge rule (`DomainProvisioningRetryRequested`) targets the existing provisioning workflow.
- **Functions**: `domain-state` (retry action), `domain-registrations` (preflight check).
- **Infra**: one EventBridge rule; the registrations consumer needs `events:DescribeEventBus` and `ssm:GetParameter` on the platform account.
- **Web console**: the admin domain list/detail gains a Retry action for failed domains (follows the existing enable/disable pattern).
- **Out of scope**: adoptive provisioning (importing pre-existing resources), owner-account collision checks via the trust role, deletion of failed domains (manual teardown remains the escape hatch).
