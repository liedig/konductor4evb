# Multi-env provisioning, three-workflow split, all-or-nothing, platform-configured env list

[Documentation home](../README.md)

- Recorded date: 2026-04-22
- Change ID: `multi-env-domain-provisioning`
- Area: Provisioning
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Every approved domain is now provisioned across the full list of environments configured on the platform (for example, `dev,staging,prod`) rather than just one.

**Platform-configured**: the list comes from a new `Environments` SAM parameter (`CommaDelimitedList`, default `dev,staging,prod`) and is NOT supplied per registration—domain requesters cannot override it. The command handler stamps the current list onto the domain record at registration time so approvals run against a stable snapshot.

**Three-workflow split**: the monolithic `ProvisioningWorkflow` is replaced by `provision-domain.asl.yaml` (outer coordinator), `provision-env.asl.yaml` (per-env resource creation with inner saga), and `deprovision-env.asl.yaml` (idempotent per-env teardown). The outer workflow fans out via a `Map` state with unbounded concurrency.

**All-or-nothing contract**: if any env's inner execution fails, the outer workflow runs `deprovision-env` over every env that had succeeded and moves the domain to `PROVISIONING_FAILED`. `DomainProvisioningFailed` carries `failedEnvironment`, `failedStep`, `errorCode`, `errorMessage`, and aggregated `compensationErrors`.

**Namespace + RAM at domain level**: the SSM parameter (`/namespace/{domain}`) and RAM share are created once above the Map and are never rolled back—they are env-independent and safe to reuse on retry.

**Trust role env-independent**: the cross-account trust role is renamed from `{org}-{env}-konductor-trust-role` to `{org}-konductor-trust-role` with resource scopes broadened from `{org}-{env}-*` to `{org}-*`, so one role per domain account covers every env.

**Parameter rename**: the `OrganizationArn` SAM parameter was renamed to `AwsOrganizationArn` to disambiguate the AWS Organizations service ARN from the `Organization` short identifier used in Konductor resource naming—the two had almost identical names but are unrelated concepts. `deprovision-env` is intentionally reusable as the future `DELETE /domains` primitive.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-04-27-multi-env-domain-provisioning/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
