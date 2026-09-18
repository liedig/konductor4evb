## 1. Enforcement contract and fixtures

- [ ] 1.1 Produce the action/resource/condition matrix for EventBridge bus lifecycle/configuration/policies, SQS lifecycle/configuration/purge/redrive, protected tagging, IAM exception identities, and organization departure. Verify every action and condition against current AWS authorization documentation, including batch-action equivalence and unsupported SCP paths; record references and coverage limits.
- [ ] 1.2 Add failing table-driven infrastructure fixtures for ordinary application principals, local administrators, the exact Konductor trust-role session, central recovery, and similarly named unauthorized roles. Verify the matrix includes allowed integration operations, denied infrastructure changes, unrelated-resource controls, tag/name bypasses, and protected-role escalation attempts.

## 2. Organization guardrail package

- [ ] 2.1 Create the separately deployable organization-admin policy/template package with explicit account/OU targets and protected resource selectors. Verify local template and policy assertions cover existing and future reserved resource names without denying unrelated resources or adding organization permissions to runtime handlers.
- [ ] 2.2 Implement narrowly scoped bus/queue mutation and queue-purge denies while preserving team-owned rule/target operations and queue consumption. Verify the positive and negative fixtures from 1.2 pass, including absent/forged tags, reserved-name creation, and batch equivalents.
- [ ] 2.3 Protect the exact trust and recovery identities from local trust/policy changes, policy attachment/version manipulation, deletion/recreation, passing, boundary changes, and indirect deployment-role escalation. Verify ordinary Konductor sessions cannot modify their own trust and that only the centrally configured maintenance path can perform authorized role upgrades.
- [ ] 2.4 Provide central recovery/bootstrap and attachment inputs, including prevention of unauthorized organization departure. Verify package validation rejects incomplete exception/target configuration, has no broad principal-name exception, and documents management-account/service-linked-role exclusions.
- [ ] 2.5 Verify provisioning, compensation, deprovisioning, retagging, and integration-DLQ delivery authorization against the enforcement matrix. If the integration DLQ needs additional trusted queue-policy setup, add only that scoped provisioning support with tests proving teams need no queue-policy write access; classify any changed handler write path in the side-effects decision record.

## 3. Domain detail resource references

- [ ] 3.1 Add failing isolated UI tests for the six provisioned resource fields, mixed accounts, co-located platform/domain accounts, malformed and unknown identifiers, missing values, and queue URLs. Verify the tests require platform identifiers to remain visible without links and only recognized domain-account application resources to be linked.
- [ ] 3.2 Update the resource classification and console-link eligibility in the domain detail page and utility without changing API responses. Verify tests from 3.1 pass, identifiers remain read-only, ownership groups are clear, and navigation is not described as enforcing read-only AWS access.
- [ ] 3.3 Validate keyboard navigation and narrow-screen rendering of the revised resource groups using the existing Playwright harness. Run `rtk pnpm lint`, `rtk proxy pnpm typecheck`, the focused UI suite, and `rtk pnpm build` from `web/`; record unrelated failures separately.

## 4. Operational evidence and documentation

- [ ] 4.1 Write a repeatable sandbox acceptance procedure with isolated fixtures and explicit positive/negative cases for application operations, administrator denial, role bypass attempts, Konductor lifecycle operations, and central recovery. Verify the procedure captures policy versions, targets, observed AWS outcomes, cleanup scope, and SCP exclusions without embedding credentials or performing live writes.
- [ ] 4.2 Update `docs/development/decisions.md` with the resource ownership boundary, SCP placement, protected exceptions, recovery model, and any changed side-effect classifications. Verify it distinguishes the new organization enforcement from the earlier trust-only design.
- [ ] 4.3 Update `docs/development/architecture.md` and installation guidance with the two-account resource map, separate IAM grants versus SCP ceilings, recovery-first bootstrap, staged attachment, and rollback procedure. Verify the guidance does not claim protection for management-account or service-linked-role paths or equate console links with authorization.
- [ ] 4.4 After separate explicit authorization, run the sandbox acceptance procedure and record actual AWS outcomes. Leave this task unchecked when authorization or a sandbox is unavailable; local fixture success is not evidence of deployed SCP enforcement.

## 5. Final gates

- [ ] 5.1 Run the repository's relevant infrastructure test suites, `rtk cargo fmt --all -- --check`, applicable clippy checks, core/trust SAM validation, and validation appropriate to the new organization template. Verify all changed policy paths are covered and report pre-existing failures separately.
- [ ] 5.2 Run `rtk proxy openspec validate protect-domain-managed-resources --strict` and review the scoped diff. Verify no automatic organization attachment, unrelated runtime feature, customer-specific identifiers, credentials, or changes to metadata/schema-gate work are included; report local completion separately from the authorized sandbox gate.
