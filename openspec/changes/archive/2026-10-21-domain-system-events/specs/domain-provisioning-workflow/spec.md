# Spec Delta

## ADDED Requirements

### Requirement: Domain-level system-events topic provisioning

Provisioning SHALL create and configure one system-events topic at domain scope before marking the domain `ACTIVE`. The topic policy SHALL use the registered owner and effective environment accounts and the system-event protocol restrictions. Its ARN SHALL be persisted as top-level `systemEventsTopicArn`, outside per-environment `resourceArns`.

#### Scenario: All environments share the provisioned topic
- **WHEN** a domain with several environments completes provisioning
- **THEN** topic creation and policy configuration SHALL have succeeded once at domain scope
- **AND** the domain record SHALL contain its topic ARN independently of each environment's resource map

#### Scenario: Repeated provisioning converges on the same managed topic
- **WHEN** the same provisioning execution retries topic creation or policy application
- **THEN** it SHALL reuse the deterministically named, verified platform-managed topic
- **AND** it SHALL converge on the same policy without creating another topic

#### Scenario: Topic name belongs to an unrelated resource
- **WHEN** the expected name is already used by a topic whose ownership cannot be verified
- **THEN** provisioning SHALL fail visibly rather than adopt, retag, repolicy, or delete that resource

### Requirement: Topic provisioning compensation

A new-domain provisioning failure SHALL compensate its system-events topic alongside successful environment resources. Topic deletion SHALL be idempotent, clear any recorded ARN only after successful deletion, and collect genuine errors without preventing the terminal failure status. Namespace and RAM resources SHALL retain their existing retry behavior.

#### Scenario: Policy configuration fails after topic creation
- **WHEN** topic creation succeeds but policy configuration exhausts retries
- **THEN** provisioning SHALL delete the topic it owns and reach `PROVISIONING_FAILED`
- **AND** it SHALL NOT report a ready topic ARN

#### Scenario: Environment failure rolls back the shared topic
- **WHEN** an environment fails after the shared topic was created
- **THEN** rollback SHALL compensate both successful sibling environments and the shared topic
- **AND** a topic deletion error SHALL be included in the existing compensation error reporting

#### Scenario: Repeated compensation finds no topic
- **WHEN** rollback is retried after the topic has already been deleted
- **THEN** absence SHALL count as success, not an additional compensation failure

### Requirement: Non-destructive topic backfill

The platform SHALL provide an operator-run, idempotent backfill for existing `ACTIVE` and `INACTIVE` domains. It SHALL create or reconcile only the domain's managed topic, policy, tags, and recorded ARN. It SHALL preserve lifecycle status and business resources, paginate the domain inventory, and report failures per domain without hiding successful work.

#### Scenario: Existing active domain receives a topic
- **WHEN** the operator runs backfill for an `ACTIVE` domain with no topic ARN
- **THEN** its topic and policy SHALL be established and its ARN recorded
- **AND** its buses, registries, queues, subscriptions, and lifecycle status SHALL remain unchanged

#### Scenario: Inactive domain is not re-enabled
- **WHEN** backfill processes an `INACTIVE` domain
- **THEN** its topic SHALL be established without enabling the domain or any subscription

#### Scenario: Backfill is retried after partial completion
- **WHEN** backfill previously configured a topic but failed before recording its ARN
- **THEN** a retry SHALL verify and reuse the topic, preserve existing subscriptions, and record the ARN

#### Scenario: Inventory spans several pages
- **WHEN** the domain inventory exceeds one database page and one eligible domain fails
- **THEN** backfill SHALL process all pages and report that domain's failure
- **AND** pending, rejected, and provisioning-failed domains SHALL not be treated as provisioned domains

### Requirement: Default owner email subscription

Provisioning SHALL subscribe the domain's owner email address to its system-events topic as the default endpoint, idempotently and without blocking provisioning on the address owner's confirmation. A committed owner-email change SHALL reconcile the default subscription, and the backfill SHALL ensure it for existing domains.

#### Scenario: Owner email is subscribed once during provisioning
- **WHEN** a domain with an owner email completes topic provisioning
- **THEN** the topic SHALL have one email subscription for that address
- **AND** the subscription SHALL remain pending until the owner confirms it, which SHALL NOT fail or hold provisioning

#### Scenario: Provisioning retry does not duplicate the subscription
- **WHEN** the provisioning execution retries after the owner email was already subscribed
- **THEN** it SHALL converge on the existing subscription rather than creating a duplicate

#### Scenario: Owner email change follows the domain update
- **WHEN** a committed domain update replaces the owner email
- **THEN** the old address's default subscription SHALL be unsubscribed and the new address subscribed
- **AND** a reconciliation failure SHALL remain observable without reversing the domain metadata update

#### Scenario: Backfill establishes the owner subscription
- **WHEN** backfill processes an existing domain whose topic lacks the owner email subscription
- **THEN** it SHALL subscribe the current owner email and leave other endpoint subscriptions untouched

### Requirement: System-events topic tags follow domain ownership

Topics SHALL carry `konductor:managed-by`, `konductor:domain`, `konductor:namespace`, and the domain's `cost-center` when configured, without an environment tag. A committed cost-center change SHALL retag the recorded topic through the existing retag path. Missing topic ARNs during rollout SHALL be skipped, not inferred or treated as fatal.

#### Scenario: Topic is tagged at creation
- **WHEN** provisioning or backfill creates a topic for a domain with a cost center
- **THEN** it SHALL apply the domain-level managed tags and cost center
- **AND** tag failure SHALL prevent reporting the topic as ready

#### Scenario: Cost center changes after backfill
- **WHEN** a committed cost-center update triggers retagging for a domain with a recorded topic
- **THEN** retagging SHALL update that topic as well as the existing resources
- **AND** retag failure SHALL remain observable without reversing the domain metadata update

#### Scenario: Domain is disabled after provisioning
- **WHEN** a provisioned domain becomes `INACTIVE`
- **THEN** its topic and SNS subscriptions SHALL remain in place like its other retained infrastructure
