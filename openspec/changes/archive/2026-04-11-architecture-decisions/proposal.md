## Why

This is a greenfield project with no source code yet. Before implementation begins, we need to document binding architecture decisions that shape the entire system: tech stack choices, resource topology across AWS accounts, workflow design, and the cross-account trust model. These decisions were made through collaborative exploration of the BRD, PRFAQ, and the re:Invent API311 presentation architecture, and must be captured as the authoritative reference for all subsequent implementation work.

## What Changes

- Establish the control plane tech stack: Rust on AWS Lambda (cargo-lambda, provided.al2023), API Gateway REST, DynamoDB (table-per-entity), AWS SAM, OpenAPI
- Define the resource topology: what the control plane provisions in the platform account vs. application accounts, including domain event buses, PutEvent policies, schema registries, DLQs, and observability resources
- Design the domain registration workflow: two-phase Step Functions approach (approval workflow with human-in-the-loop, then provisioning workflow), with lifecycle events on a dedicated control plane event bus
- Specify the cross-account trust model: assumed roles in application accounts, permission boundaries, and the installation story for platform teams onboarding new accounts
- Define the domain namespace convention: configurable `{env}-{org}-{domain}` pattern used for resource naming across both accounts, with support for multiple environments (dev, staging, prod)

## Capabilities

### New Capabilities

- `system-architecture`: Tech stack, runtime, framework, database, IaC, and API design decisions for the control plane
- `resource-topology`: What AWS resources the control plane creates and manages per domain, in which accounts, and the 1:1 domain-bus-per-subscribing-account model
- `domain-provisioning-workflow`: Two Step Functions workflows (approval + provisioning), human-in-the-loop approval, configurable namespace convention, resource creation sequence across both accounts
- `cross-account-trust-model`: IAM assumed roles in application accounts, permission requirements for each resource type, installation story (trust stack per account), bus-to-bus routing roles

### Modified Capabilities

- `domain-management`: The existing spec describes the domain lifecycle but does not specify the resource topology, namespace convention, or cross-account provisioning details. Those details are captured in the new capabilities above and will inform but not modify the domain-management spec requirements.

## Impact

- All future implementation work derives from these decisions
- The domain-management spec's provisioning scenarios (register, approve, provision, delete) will reference the resource topology and workflow designs defined here
- AWS SAM templates, DynamoDB table schemas, Lambda function structure, and API Gateway configuration all flow from the system-architecture spec
- The cross-account trust model defines a prerequisite installation step that platform teams must complete before the control plane can manage resources in their application accounts
