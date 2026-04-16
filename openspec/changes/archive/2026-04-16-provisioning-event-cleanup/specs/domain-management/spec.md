## MODIFIED Requirements

### Requirement: Domain management events

#### Events emitted by the provisioning workflow

Replace the provisioning workflow events table:

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainProvisioningSucceeded` | When the provisioning workflow completes successfully | Control plane updates domain to ACTIVE with resource ARNs | `domainId`, `workflowExecutionId`, `completedAt`, `resourceArns`, `namespaceParameter`, `isNewAccount` |
| `DomainProvisioningFailed` | When the provisioning workflow fails | Control plane updates domain to PROVISIONING_FAILED | `domainId`, `workflowExecutionId`, `failedAt`, `errorCode`, `errorMessage`, `compensationResults` |

Remove the `DomainCreationStarted` row entirely.
