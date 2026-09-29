# Operations

[Documentation home](../README.md)

These procedures are for platform and account administrators. Documentation is
not authorization to deploy, change access, activate a revision, or delete data.

## Install and onboard

1. Review [service quotas and capacity](service-quotas.md).
2. [Install Konductor](installation.md).
3. [Deploy domain-account trust](trust-stack-deployment.md).
4. [Set up users, bindings, and entitlements](access-administration.md).
5. Run the [installation smoke test](installation.md#phase-4--smoke-test).

## Operate and recover

- [Installation troubleshooting](installation-troubleshooting.md)
- [Domain recovery and cleanup](domain-recovery.md)
- [Metadata rollout and consumer sign-off](metadata-rollout.md)
- [Uninstall](uninstall.md)

## Separately authorized protection

[Organization guardrails](guardrails.md) require organization-admin approval,
recovery-first bootstrap, and sandbox evidence. They are not an ordinary
installation phase. Enforcement was verified in the authorized
[sandbox acceptance run](../../openspec/changes/archive/2026-09-29-protect-domain-managed-resources/sandbox-acceptance-evidence.md).
