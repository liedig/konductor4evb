# Design: provisioning resilience

## Context

Provisioning runs as a saga: the approval workflow emits `DomainRegistrationApproved` on the control plane bus; an EventBridge rule starts `konductor-provision-domain`, which fans out one `provision-env` execution per environment. Per-env failure compensates backwards; domain-level failure ends in `PROVISIONING_FAILED` with `errorDetail`. There is no way back from that status: re-registration rejects the name (only absent or `REJECTED` records may be replaced), and the approval-created role bindings would sabotage any re-approval anyway (finalize-approval's `attribute_not_exists` puts would cancel the whole transaction).

The `sales` incident showed the second weakness: orphaned resources from a previous era collided at the first provisioning step, after a human had already approved. The registration path never looks at the AWS estate at all.

## Goals / Non-Goals

**Goals:**
- One admin action retries provisioning for a failed domain, reusing the saga unchanged
- Registration fails fast and legibly when platform resource names collide, before anyone reviews it
- Both behaviors fail closed and audit through the existing lifecycle event stream

**Non-Goals:**
- Adoptive provisioning (importing pre-existing resources into the saga)
- Owner-account collision checks via the trust role (platform-account names cover the observed failure class; owner-account names share the same namespace stem and their absence was verified in the incident, but checking them requires cross-account machinery the registrations consumer does not have)
- Admin deletion of failed domains (manual teardown script remains)
- Surfacing retry in the IAM API surface

## Decisions

1. **Retry is a new action on the existing state route.** `POST /domains/{domainName}/state` already owns platform-admin lifecycle transitions (`enable`/`disable`) with the same authorization shape; `retry-provisioning` joins it rather than inventing a route. Alternative considered: a dedicated `POST /domains/{domainName}/retry` route — rejected; the route surface stays small and the state handler already has the record and the bus client.

2. **Retry re-enters provisioning via a new event, not a replayed one.** The handler conditionally updates `PROVISIONING_FAILED → APPROVED` (clearing `errorDetail`) and emits `DomainProvisioningRetryRequested` with the registration fields read from the domain record. A new rule on the control plane bus starts the provisioning workflow for that detail-type. Replaying `DomainRegistrationApproved` would falsify the audit trail (a second "approval" that never happened); a distinct detail-type keeps the lifecycle honest while the workflow input shape stays identical. The conditional write guards the race where two admins retry concurrently — one wins, the other gets `409`.

3. **Retry never touches bindings.** The bindings from the original approval are intact and correct; the transaction model that creates them lives in finalize-approval, which retry does not invoke.

4. **Preflight collision check lives in the registrations consumer, not the API.** The register API is an SQS queue with a 202 contract — no Lambda runs synchronously, so "request time" means during command processing, before the record is written. The check is `events:DescribeEventBus` per configured environment plus `ssm:GetParameter` on `/namespace/{domain}`, platform account only, using the consumer's own execution role.

5. **Collision outcome is a system rejection, not a new status.** The record persists as `REJECTED` with `decidedBy: system` and a justification naming the colliding resources. This composes with three existing behaviors for free: the requester sees `REJECTED` in their domains list (team-request visibility), the name is not blocked (the conditional put replaces `REJECTED` records), and no approval workflow starts. A dedicated status (e.g. `CONFLICT`) would churn the status model, the UI, and every status filter for no new information.

6. **Check errors fail the command, loudly.** A throttled/denied DescribeEventBus is not proof of absence; the handler returns the error so the message follows the queue's redrive to the DLQ and an operator investigates, rather than writing a record on a guess.

## Risks / Trade-offs

- **Retry while the failed saga's stragglers exist**: if the failed run's compensation left resources behind, the retry collides exactly like the original. Acceptable: compensation is the documented contract, and the retry failure surfaces the same `errorDetail` for manual cleanup. The preflight check reduces how often anyone reaches this state.
- **Preflight adds two AWS calls per registration** (per env + one SSM): negligible at registration volumes (human-paced), both strongly consistent reads.
- **Platform-only check misses owner-account collisions**: a stale app bus in the owner account would still fail at provision time. Documented non-goal; can be added later by assuming the trust role from the consumer if it recurs.
- **`REJECTED` for a machine decision** slightly overloads the admin-decision semantics; mitigated by `decidedBy: system` and the justification text, which the UI renders verbatim.
