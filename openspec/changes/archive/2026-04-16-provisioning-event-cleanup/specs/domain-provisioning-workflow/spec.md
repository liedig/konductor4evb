## MODIFIED Requirements

### Requirement: Provisioning workflow resource creation

#### Scenario: Provisioning workflow creates all resources

Update step 8 outcome: replace "the workflow SHALL emit a `DomainCreationSucceeded` event" with "the workflow SHALL emit a `DomainProvisioningSucceeded` event".

#### Scenario: Provisioning workflow step failure triggers saga rollback

Replace "the workflow SHALL emit a `DomainCreationFailed` event" with "the workflow SHALL emit a `DomainProvisioningFailed` event".

### Requirement: Provisioning workflow lifecycle events

Replace the entire requirement text:

The provisioning workflow SHALL emit lifecycle events to the Nexus event bus for provisioning outcomes only. No intermediate events SHALL be emitted. The event source SHALL be `nexus.domain-management`.

#### Scenario: Provisioning started event

REMOVE this scenario entirely.

#### Scenario: Provisioning succeeded event

Replace: emit a `DomainCreationSucceeded` event → emit a `DomainProvisioningSucceeded` event. Add `isNewAccount` flag to the payload.

- **WHEN** the provisioning workflow completes all steps successfully
- **THEN** it SHALL emit a `DomainProvisioningSucceeded` event with source `nexus.domain-management`, domainId, workflowExecutionId, completedAt, resourceArns (map of all created resource ARNs), namespaceParameter (SSM parameter path), and isNewAccount flag

#### Scenario: Provisioning failed event

Replace: emit a `DomainCreationFailed` event → emit a `DomainProvisioningFailed` event.

- **WHEN** the provisioning workflow fails at any step
- **THEN** it SHALL emit a `DomainProvisioningFailed` event with source `nexus.domain-management`, domainId, workflowExecutionId, failedAt, errorCode, errorMessage, and compensationResults (list of cleanup outcomes)
