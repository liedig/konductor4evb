# Single domain table, sync decide, workflow-owned events

[Documentation home](../README.md)

- Recorded date: 2026-04-14
- Change ID: `domain-management-redesign`
- Area: Domain Management
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Merged domains + registration requests into one table. Decide endpoint is synchronous (API GW → Lambda, no SQS). Approval workflow solely owns approval/rejection events and DDB status updates. All Step Functions use YAML + JSONata. SES notifications for approval requests and rejections. Update command is metadata-only (no status transitions). Status model: PENDING_APPROVAL → APPROVED → PROVISIONING → ACTIVE, with REJECTED, PROVISIONING_FAILED, INACTIVE.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-04-14-domain-management-redesign/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
