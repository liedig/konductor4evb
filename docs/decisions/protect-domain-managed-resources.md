# Organization SCP guardrails for Konductor-managed resources; ownership-aware domain resource presentation

[Documentation home](../README.md)

- Recorded date: 2026-09-16
- Change ID: `protect-domain-managed-resources`
- Area: Trust / Web UI
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Resource ownership boundary**: the domain detail page now classifies provisioned resources by purpose first (platform vs domain account) and verifies account identity second—platform resources (platform bus, registry, subscription DLQ, dashboard) stay text-only even in a co-located deployment, and only application-bus / integration-DLQ identifiers whose ARN or queue URL resolves to the domain's `ownerAccountId` get console links. Malformed, missing, or mismatched identifiers fall back to text without inventing a URL, and links are never presented as read-only AWS sessions.

**SCP placement**: guardrails ship as a separate organization-admin package (`guardrail-stack.yaml`) with explicit OU/account targets—not in `konductor-core-stack.yaml` and not in `trust-stack.yaml`—so ordinary deployments gain no organization-wide authority, and attachment requires separate authorization.

**Protected exceptions**: the resource-management denies exempt exactly two principals by exact ARN—the Konductor trust role and a centrally configured recovery role. No wildcard principal patterns (`*konductor*`) anywhere; the trust role is NOT exempt from identity protection (it cannot rewrite its own trust policy), and role-name recreation (including `-suffixed` look-alikes) is denied to everyone except the recovery path.

**Recovery model**: centrally controlled, bootstrapped and tested BEFORE guardrails attach (recovery-first); central SCP maintenance remains the ultimate fallback, and rollback means detach → repair → re-attach, never deleting protected resources.

**Enforcement contract**: the action/resource/condition matrix (`enforcement-matrix.md`) protects bus/queue lifecycle, configuration, policy replacement, purge, and protected tagging while deliberately leaving team-owned rule/target management and DLQ messaging (receive/delete/visibility, including batch equivalents) to ordinary team IAM. Durable anchors are ARN/name patterns, not tags alone.

**Distinction from the earlier trust-only design**: `trust-stack.yaml` authorizes Konductor (an allow grant to platform roles) but does nothing to stop a domain-account administrator from deleting or reconfiguring the managed infrastructure; the SCPs add the missing deny ceiling over ALL member-account principals. SCPs remain ceilings, not grants—team roles still need their ordinary IAM allows.

**Side-effect classifications**: this change adds NO handler write paths (infrastructure template + UI presentation only), so the stream-driven side-effects audit table below is unchanged.

**Coverage limits documented, not assumed away**: SCPs do not apply to management-account principals or service-linked roles; enforcement claims are valid only after the authorized sandbox acceptance procedure (`sandbox-acceptance.md`) passes—local fixtures prove policy shape, not deployed enforcement.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-09-29-protect-domain-managed-resources/proposal.md). The recorded date above is not necessarily the archive or merge date.

## Current relevance

Implementation is merged. The [account-selector discrepancy](../reference/documentation-discrepancies.md#d09) was resolved on 2026-09-29, when the authorized sandbox acceptance run recorded enforcement. Each installation still verifies enforcement in a sandbox before production attachment; see the [guardrail runbook](../operations/guardrails.md#verification--local-fixtures-first-then-authorized-sandbox).

[Decision index](README.md)
