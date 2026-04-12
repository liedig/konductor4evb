## 1. Project Scaffolding

- [x] 1.1 Initialize Rust workspace with cargo-lambda (Cargo.toml workspace with member crates)
- [x] 1.2 Create shared library crate for common types, error definitions (thiserror), and DynamoDB helpers
- [x] 1.3 Create initial SAM template with Globals, Parameters (environment, org, namespace convention), and Outputs
- [x] 1.4 Define OpenAPI specification skeleton with the first operations (RegisterDomain, ListDomains, DescribeDomain)
- [x] 1.5 Configure cargo-lambda build targets and SAM build integration

## 2. Control Plane Core Infrastructure (SAM)

- [x] 2.1 Define the control plane EventBridge event bus in SAM template
- [x] 2.2 Define DynamoDB tables for domains and registration requests (table-per-entity, initial key schema)
- [x] 2.3 Define IAM execution roles for Lambda functions (platform account permissions: EventBridge, DynamoDB, SQS, CloudWatch, Schemas, STS AssumeRole)
- [x] 2.4 Define API Gateway REST API resource with IAM authorization, referencing the OpenAPI spec
- [x] 2.5 Define EventBridge rules on the control plane bus for workflow triggering (DomainRegistrationApproved -> provisioning workflow)

## 3. Domain Registration API

- [x] 3.1 Implement RegisterDomain Lambda function (validate input, persist to DynamoDB with PendingApproval status, start approval workflow)
- [x] 3.2 Implement ListDomains Lambda function (query DynamoDB domains table with filters and pagination)
- [x] 3.3 Implement DescribeDomain Lambda function (query domain metadata, resource ARNs, status)
- [x] 3.4 Implement ListPendingDomainActivations Lambda function (query registration requests table filtered by status)
- [x] 3.5 Implement ApproveDomainRegistration Lambda function (accept task token + decision, send task success/failure to Step Functions)

## 4. Approval Workflow (Step Functions)

- [x] 4.1 Define approval workflow state machine in SAM template (start -> wait for task token -> emit approved/rejected event)
- [x] 4.2 Implement the task token persistence step (store token in DynamoDB registration requests table)
- [x] 4.3 Implement the approval/rejection event emission step (put event to control plane bus with DomainRegistrationApproved or DomainRegistrationRejected)
- [x] 4.4 Implement DynamoDB status update on approval/rejection

## 5. Provisioning Workflow (Step Functions)

- [x] 5.1 Define provisioning workflow state machine in SAM template (triggered by DomainRegistrationApproved event)
- [x] 5.2 Implement step: create domain event bus in platform account with PutEvent policy restricting to owner account
- [x] 5.3 Implement step: create schema registry in platform account with access policies
- [x] 5.4 Implement step: create subscription rule DLQ (SQS) in platform account
- [x] 5.5 Implement step: assume role in application account and create domain event bus
- [x] 5.6 Implement step: assume role in application account and create integration rule DLQ (SQS)
- [x] 5.7 Implement step: create CloudWatch dashboard and alarms for domain bus in platform account
- [x] 5.8 Implement step: update DynamoDB domain record to Active with all resource ARNs
- [x] 5.9 Implement lifecycle event emission (DomainCreationStarted, DomainCreationSucceeded, DomainCreationFailed)
- [x] 5.10 Implement error handling: emit DomainCreationFailed with error details on step failure

## 6. Domain Namespace Convention

- [x] 6.1 Implement namespace convention resolver (accepts env, org, domain and produces resource names according to configured pattern)
- [x] 6.2 Define storage location for namespace convention configuration (SSM Parameter Store or DynamoDB config table)
- [x] 6.3 Integrate namespace resolver into provisioning workflow for all resource naming

## 7. Cross-Account Trust Stack

- [x] 7.1 Create SAM/CloudFormation template for the application account trust stack (IAM role with trust policy and scoped permissions)
- [x] 7.2 Parameterize the trust stack template (control plane execution role ARN, namespace prefix, environment)
- [x] 7.3 Document trust stack deployment instructions for platform teams

## 8. Domain Lifecycle APIs (Update, Delete)

- [x] 8.1 Implement UpdateDomain Lambda function (update mutable metadata, suspend/resume, reject immutable field changes)
- [x] 8.2 Implement DeleteDomain Lambda function (validate no dependencies, initiate soft-delete, emit DomainDeletionRequested)
- [x] 8.3 Define deletion cleanup workflow (Step Functions or Lambda) for resource teardown across both accounts

## 9. Update OpenSpec Project Context

- [x] 9.1 Update openspec/config.yaml with project context from project-bak.md (tech stack, conventions, domain context)
- [x] 9.2 Archive this change once all decisions are reviewed and confirmed
