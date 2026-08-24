# Tasks: provisioning resilience

## 1. Retry action (domain-state)

- [x] 1.1 Test: `retry-provisioning` on a `PROVISIONING_FAILED` domain transitions to `APPROVED`, clears `errorDetail`, and emits `DomainProvisioningRetryRequested` with the record's registration fields
- [x] 1.2 Test: retry on any other status returns `409` and emits nothing; team caller gets `403`
- [x] 1.3 Implement the action in `domain-state` (conditional update, event via the control plane bus)
- [x] 1.4 Template: EventBridge rule `DomainProvisioningRetryRequested` → `konductor-provision-domain` state machine; `events:PutEvents` already in place — verify the rule input transformer matches the approval rule's shape

## 2. Preflight collision check (domain-registrations)

- [x] 2.1 Test: colliding platform event bus or namespace parameter → record persisted as `REJECTED`, `decidedBy: system`, justification names the resources, no approval workflow started, `RegisterDomainFailed` emitted
- [x] 2.2 Test: check error (throttling) → command fails, no record written
- [x] 2.3 Test: no collision → unchanged happy path
- [x] 2.4 Implement the check in the register path (per-env `DescribeEventBus` + `ssm:GetParameter` on `/namespace/{domain}`)
- [x] 2.5 Template: registrations consumer role gains `events:DescribeEventBus` and `ssm:GetParameter`

## 3. Console

- [x] 3.1 Admin domain list/detail shows a Retry action when status is `PROVISIONING_FAILED`, with the result surfacing via the existing toast/polling pattern

## 4. Gates and verification

- [x] 4.1 `make check`, workspace tests green
- [x] 4.2 `openspec validate provisioning-resilience --strict`
- [x] 4.3 Deploy core; live-verify: register `sales` (clean path), approve, then force a collision case with a leftover-named dummy request to see the system rejection in the requester's list
