## REMOVED Requirements

### Requirement: Cleanup workflow for domain deletion

**Reason**: Domain deletion is removed from the system. Domains can only be deactivated, which preserves all provisioned resources in place. There is no resource teardown workflow needed. The `CleanupWorkflow` Step Functions state machine, its IAM role (`CleanupWorkflowRole`), and the `DomainDeletionRequested` EventBridge rule are all removed.

**Migration**: Use the UpdateDomain API with `desiredStatus: DEACTIVATED`. Deactivation disables event routing by removing PutEvent permissions on domain event buses but does not delete any AWS resources. No cleanup workflow is triggered.
