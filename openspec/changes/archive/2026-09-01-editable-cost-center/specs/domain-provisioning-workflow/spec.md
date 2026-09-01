# domain-provisioning-workflow Specification Delta

## ADDED Requirements

### Requirement: Provisioned-resource retagging on cost-center change

When a committed domain update changes `costCenter`, the platform SHALL converge the `cost-center` tag on every provisioned resource of that domain to the new value. An EventBridge rule on the control plane bus SHALL trigger a dedicated retag workflow for `UpdateDomainSucceeded` events whose `changedFields` contain `costCenter`; all other update events SHALL be ignored by the rule. The workflow SHALL retag every resource recorded in the domain record's per-environment `resourceArns` map (event buses, schema registries, DLQs, dashboards, and alarms) and the domain-level namespace parameter, applying the standard platform tag set with the new `cost-center` value. Resources in the owner's account SHALL be retagged through the `konductor-trust-role`; platform-account resources SHALL be retagged directly. The workflow SHALL be defined in YAML with JSONata as the query language, SHALL tolerate event redelivery by relying on the upsert semantics of the tagging APIs (a duplicate execution re-applies the same tag set), and SHALL emit a failure event on the control plane bus when retagging cannot be completed. Retag failure SHALL NOT roll back the committed domain record update; the record remains the system of record for the intended tag state.

#### Scenario: Cost-center change triggers retagging

- **WHEN** an `UpdateDomainSucceeded` event with `costCenter` in `changedFields` is published to the control plane bus
- **THEN** the retag workflow SHALL start
- **AND THEN** the workflow SHALL retag each resource in the domain's per-environment `resourceArns` map and the namespace parameter with the new `cost-center` value

#### Scenario: Non-cost-center updates do not trigger retagging

- **WHEN** an `UpdateDomainSucceeded` event is published whose `changedFields` do not contain `costCenter`
- **THEN** the retag workflow SHALL NOT start

#### Scenario: Redelivered event converges to the same tag state

- **WHEN** the same `UpdateDomainSucceeded` event (same domain and `requestId`) is delivered more than once
- **THEN** each delivery MAY start a workflow execution (rule targets cannot set execution names, so executions are auto-named)
- **AND THEN** every execution SHALL re-apply the same tag values, because the tagging APIs used are upserts

#### Scenario: Owner-account resources are retagged cross-account

- **WHEN** the retag workflow processes an environment
- **THEN** resources recorded in the owner's account SHALL be retagged using the `konductor-trust-role` in that account
- **AND THEN** platform-account resources SHALL be retagged without assuming the trust role

#### Scenario: Retag failure is surfaced

- **WHEN** the retag workflow cannot retag a resource (for example, a missing trust role or throttling that exhausts retries)
- **THEN** the workflow SHALL emit a failure event on the control plane bus identifying the domain and the failed step
- **AND THEN** the domain record's committed `costCenter` value SHALL remain unchanged by the failure
