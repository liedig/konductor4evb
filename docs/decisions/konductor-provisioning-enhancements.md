# Rebrand to Konductor for Amazon EventBridge, naming convention flip, saga rollback, account detection, RAM namespace sharing

[Documentation home](../README.md)

- Recorded date: 2026-04-16
- Change ID: `konductor-provisioning-enhancements`
- Area: Cross-cutting
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Rebrand**: Renamed from "EventBridge Control Plane" to "Konductor for Amazon EventBridge"—reflects that it's an integration platform, not a control/data plane split. Event source prefix changed to `konductor.domain-management`, bus renamed to `konductor-bus`, trust role to `konductor-trust-role`.

**Naming convention**: Flipped from `{env}-{org}-{domain}` to `{org}-{env}-{domain}` so resources group by org/domain rather than environment. Domain namespace defined as `{org}-{domain}` (environment-independent).

**Saga rollback**: Provisioning workflow now implements compensating transactions—on failure, walks backwards through created resources and deletes them before marking PROVISIONING_FAILED. Best-effort cleanup with error tracking.

**Account detection**: Provisioning workflow queries `by-owner` GSI to detect first-time accounts; approval notification email flags new accounts with trust-stack deployment advisory.

**RAM sharing**: Domain namespaces stored as SSM Advanced Parameters at `/namespace/{domain}`, shared org-wide via AWS RAM resource share. Domain accounts can discover domains locally without cross-account IAM.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-04-16-konductor-provisioning-enhancements/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
