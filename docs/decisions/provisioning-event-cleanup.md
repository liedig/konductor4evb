# Remove intermediate event, rename outcome events to Provisioning

[Documentation home](../README.md)

- Recorded date: 2026-04-16
- Change ID: `provisioning-event-cleanup`
- Area: Provisioning
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Removed `DomainCreationStarted` intermediate event—no consumers, and `PROVISIONING` domain status already signals that provisioning is underway. Renamed outcome events from `DomainCreationSucceeded/Failed` to `DomainProvisioningSucceeded/Failed`—the workflow provisions infrastructure, it doesn't create the domain (that happens during registration). Aligns with existing `PROVISIONING` / `PROVISIONING_FAILED` status values. Moved `isNewAccount` flag from the removed started event to the succeeded event payload.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-04-16-provisioning-event-cleanup/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
