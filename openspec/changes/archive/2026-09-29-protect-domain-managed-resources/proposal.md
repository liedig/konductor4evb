## Why

Domain details currently link platform-account resources to AWS consoles that domain owners cannot normally access. More importantly, the trust stack authorizes Konductor to provision application-account resources but does not stop domain-account administrators from deleting or reconfiguring them.

## What Changes

- Keep all provisioned resource identifiers visible as read-only reference information. Separate platform-account resources from resources in the domain account; provide console links only for recognized domain-account resources.
- **BREAKING**: Introduce centrally administered AWS Organizations SCP guardrails for participating member accounts. Domain administrators, including member-account root within SCP coverage, must not delete or reconfigure Konductor-managed application buses and integration DLQs.
- Preserve team ownership of integration rules and targets and normal queue consumption, including message acknowledgement and visibility changes. Queue purge is prohibited by default.
- Protect the exact exempt Konductor trust role against local modification, replacement, policy manipulation, and passing to other services. Protect ownership identifiers against removal or forgery that bypasses enforcement.
- Provide an explicitly configured, centrally controlled recovery path and an action/resource/condition test matrix. SCPs restrict permissions; application IAM grants remain separately necessary.
- Package guardrails separately from normal platform/domain provisioning. Attachment, live enforcement tests, and deployment require separate authorization; there is no automatic organization-policy mutation from domain registration.

## Capabilities

### New Capabilities

None. Extend existing UI, topology, and trust capabilities.

### Modified Capabilities

- `web-ui`: Ownership-aware domain resource presentation and safe console-link eligibility.
- `resource-topology`: Explicit division between team-managed integration operations and protected Konductor infrastructure.
- `cross-account-trust-model`: Organization-enforced member-account protection, protected role exceptions, and controlled recovery.

## Impact

- UI: `web/app/pages/domains/[name].vue`, `web/app/utils/consoleLinks.ts`, and isolated Playwright fixtures/tests.
- Infrastructure: a separate organization-admin guardrail template/policy package; review `trust-stack.yaml` and provisioning, deprovisioning, and retagging workflows for least-privilege compatibility.
- Verification: policy fixtures and template checks plus separately authorized AWS sandbox tests. Local assertions alone cannot prove live SCP enforcement.
- Documentation: installation/rollout guidance, ownership and enforcement architecture, and the decision log.
- No new application API, runtime organization-management permissions, or customer-configuration framework. Existing metadata composition and schema-compatibility work remains independent.
- Enforcement excludes the Organizations management account and other AWS-documented SCP exemptions, including service-linked roles. Organization administrators remain trusted. Do not claim protection against every operation originating from an exempt service path.
