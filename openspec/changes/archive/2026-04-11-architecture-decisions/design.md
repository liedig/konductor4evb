## Context

This is a greenfield AWS EventBridge control plane with no source code. The BRD and PRFAQ define a service that abstracts cross-account EventBridge complexity for platform and application teams. The re:Invent API311 presentation established the resource topology pattern: the control plane lives in a platform account, manages domain event buses and supporting resources in both the platform and application accounts, and routes events cross-account via subscription rules with assumed roles.

Before writing any code, all foundational architecture decisions must be documented and agreed upon. These decisions constrain and inform every subsequent implementation task.

## Goals / Non-Goals

**Goals:**

- Establish the canonical reference for all technology, infrastructure, and architecture choices
- Define the complete resource topology across platform and application accounts
- Specify the two-phase domain provisioning workflow (approval + resource creation)
- Document the cross-account trust model and onboarding story for application accounts
- Provide enough architectural clarity that implementation can proceed without ambiguity on foundational questions

**Non-Goals:**

- Detailed API schema design (OpenAPI spec will be created during implementation)
- DynamoDB table schemas, partition key design, or GSI definitions (will evolve with implementation)
- Subscription management workflow design (out of scope for this change; only domain management is covered)
- Console/UI design for the approval interface
- CI/CD pipeline design
- Multi-region deployment strategy

## Decisions

### D1: Rust on Lambda with cargo-lambda and provided.al2023

**Decision**: Use Rust compiled with cargo-lambda, deployed on the `provided.al2023` custom runtime. Raw Lambda event handler (no HTTP framework adapter like axum).

**Rationale**: Rust provides fast cold starts (~10-50ms), low memory footprint, and strong type safety. The `provided.al2023` runtime is the current Amazon Linux 2023 base. Raw handler avoids unnecessary abstraction for Lambda-to-Lambda communication. cargo-lambda is the standard Rust Lambda toolchain.

**Alternatives considered**:

- *TypeScript/Node.js*: Faster development velocity but weaker type safety and higher cold starts. Rejected because the BRD targets a production AWS service where performance and reliability are paramount.
- *Go*: Similar performance to Rust but less expressive type system. Rejected in favor of Rust's ownership model and ecosystem.
- *HTTP framework adapter (axum/actix)*: Would allow local development with a web server. Rejected to keep the Lambda handler minimal and avoid framework coupling, though this could be revisited if local dev experience becomes a bottleneck.

### D2: One Lambda function per API operation

**Decision**: Each REST API operation (RegisterDomain, ListDomains, DescribeDomain, etc.) gets its own Lambda function as a separate binary target.

**Rationale**: Clean separation of concerns, independent scaling, independent deployment, and minimal cold start per function (smaller binary). Rust cold starts are fast enough that the per-function overhead is negligible.

**Alternatives considered**:

- *Shared router function*: Single Lambda handling all routes. Simpler deployment but larger binary, longer cold starts, and blast radius of a bug affects all operations. Rejected for a service with 20+ operations.
- *Module-per-route in shared function*: Compromise approach. Rejected because Rust binary sizes grow significantly with all dependencies bundled.

### D3: API Gateway REST API with IAM authentication

**Decision**: Use API Gateway REST API type (not HTTP API) with IAM authentication. API contract defined in OpenAPI.

**Rationale**: REST API type supports IAM auth natively (SigV4), request/response validation from OpenAPI, and usage plans. This aligns with AWS service conventions. OpenAPI provides a machine-readable contract for SDK generation.

**Alternatives considered**:

- *HTTP API*: Cheaper and faster but lacks native IAM policy-based authorization and built-in request validation. Rejected because IAM auth is a hard requirement for an AWS-style control plane.
- *Lambda Function URLs*: No API management features. Rejected.

### D4: DynamoDB table-per-entity

**Decision**: Each entity type (domains, registration requests, subscriptions, schemas) gets its own DynamoDB table. Use the `aws-sdk-dynamodb` crate directly without an ORM.

**Rationale**: Simpler to reason about, easier to evolve independently, and avoids the complexity of single-table design upfront. The raw SDK avoids another abstraction layer and keeps the team close to the DynamoDB data model.

**Alternatives considered**:

- *Single-table design*: DynamoDB best practice for minimizing round trips. Rejected for now because the access patterns are not fully known yet. Can be consolidated later if query patterns demand it.
- *DynamoDB mapper/ORM*: Additional abstraction. Rejected to maintain direct control over queries and avoid mapping overhead.

**Risk**: May require refactoring to single-table if composite views (e.g., describe-domain with metadata + resources + stats) become a performance bottleneck. Accepted as a known trade-off.

### D5: Two Step Functions workflows for domain registration

**Decision**: Domain registration uses two separate Step Functions workflows: (1) an approval workflow with a human-in-the-loop Task Token wait, and (2) a provisioning workflow that creates all domain resources.

**Rationale**: Separation keeps approval logic independent from provisioning logic. The approval workflow can evolve (e.g., automated policy checks, multi-level approval) without affecting provisioning. Communication via events on the control plane bus makes the handoff observable, auditable, and loosely coupled.

**Alternatives considered**:

- *Single workflow with Task Token wait then provisioning steps*: Simpler but tightly couples approval and provisioning. A change to either half requires redeploying the entire workflow. Rejected for modularity.
- *API-driven approval (no workflow)*: Approval via a direct API call that then starts provisioning. Loses the audit trail and observability of a workflow. Rejected.

### D6: Dedicated control plane event bus

**Decision**: The control plane has its own Amazon EventBridge event bus for internal lifecycle events. This bus is not exposed to application teams.

**Rationale**: Provides a clean internal communication channel for workflow orchestration. EventBridge rules on this bus trigger downstream workflows (e.g., `DomainRegistrationApproved` triggers provisioning). This bus also serves as an audit log of all control plane state transitions.

### D7: Control plane provisions resources in both accounts

**Decision**: The control plane is the single owner of all domain infrastructure. It creates domain event buses, DLQs, schema registries, policies, and observability resources in both the platform account (directly) and application accounts (via assumed roles).

**Rationale**: Centralizing provisioning ensures consistency, enforces naming conventions, and prevents drift. Application teams never need to know about or manage domain infrastructure.

### D8: Cross-account trust via assumed roles with trust stack

**Decision**: Each application account deploys a "trust stack" (SAM/CloudFormation template) that creates an IAM role the control plane can assume. The control plane project provides this template. The trust role is scoped to domain resources matching the namespace convention.

**Rationale**: This is the standard AWS pattern for cross-account access. The trust stack is a one-time setup per account. Scoping permissions to the namespace prefix ensures least privilege.

**Alternatives considered**:

- *CloudFormation StackSets via AWS Organizations*: Would automate trust stack deployment across all accounts. Deferred for now because it requires Organizations integration and adds complexity. Can be added later as an enhancement.
- *Resource-based policies only*: Insufficient for creating resources in other accounts (need IAM role for STS assumption).

### D9: Configurable domain namespace convention

**Decision**: Resources are named using a configurable convention, defaulting to `{env}-{org}-{domain}`. Platform teams can override this pattern. The convention is applied to event buses, schema registries, DLQs, IAM roles, and CloudWatch resources across both accounts.

**Rationale**: Namespace consistency is critical for IAM policy scoping (wildcard patterns), resource discovery, and operational visibility. Environment prefixing (dev/staging/prod) ensures resource isolation. Configurability accommodates different organizational naming standards.

### D10: AWS SAM for infrastructure as code

**Decision**: All control plane infrastructure is defined in AWS SAM templates. SAM templates cover Lambda functions, API Gateway, DynamoDB tables, Step Functions, IAM roles, EventBridge buses/rules, and supporting resources.

**Rationale**: SAM is a first-party AWS tool built on CloudFormation. It has native support for Lambda, API Gateway, and Step Functions. It supports `sam local` for testing and `sam deploy` for deployment. This aligns with the serverless-first approach.

**Alternatives considered**:

- *AWS CDK*: More expressive, programmatic infrastructure. Rejected because it adds a TypeScript/Python dependency to a Rust project and introduces another abstraction layer over CloudFormation.
- *Terraform*: Cloud-agnostic. Rejected because this is an AWS-only project and SAM provides tighter integration with Lambda and API Gateway.

### D11: Error handling with thiserror + anyhow

**Decision**: Library crates use `thiserror` for typed errors. Lambda handlers use `anyhow` for internal error chains. API responses return simplified error messages.

**Rationale**: `thiserror` provides clean, typed error enums for library boundaries. `anyhow` simplifies error propagation inside handlers where the specific error type matters less than the error chain for logging. API callers never see internal details.

## Risks / Trade-offs

**[Table-per-entity may require refactoring]** → Accept the risk. Migrate to single-table design if composite queries become a performance bottleneck. The table-per-entity approach is simpler to start with and the access patterns will become clearer during implementation.

**[Cross-account assumed role adds latency]** → Each STS AssumeRole call adds ~50-100ms. The provisioning workflow runs asynchronously so this is acceptable. For subscription routing, the bus-to-bus assumed role is configured once and EventBridge caches the credential. Mitigation: monitor STS call latency.

**[Trust stack deployment is a manual prerequisite]** → Platform teams must deploy the trust stack in each application account before the control plane can manage resources there. Mitigation: provide clear documentation and a single-command deployment experience. Future enhancement: automate via StackSets.

**[Namespace convention change is forward-only]** → Changing the naming convention does not rename existing resources. This could lead to inconsistency if the convention is changed after domains are provisioned. Mitigation: document this clearly and discourage convention changes after initial setup.

**[One Lambda per operation increases deployment complexity]** → 20+ Lambda functions means 20+ build artifacts. Mitigation: cargo-lambda supports workspace builds, and SAM handles deployment of multiple functions. CI/CD pipeline will need to build all targets efficiently.

**[No HTTP framework limits local development]** → Raw Lambda handlers cannot be tested with a local web server. Mitigation: use `sam local invoke` for local testing and write integration tests against the Lambda handler directly.

## Open Questions

1. **DynamoDB table schemas**: Exact partition keys, sort keys, and GSIs for each table will be designed during the first implementation change. What queries are needed for list-domains with filters and pagination?
2. **Namespace convention storage**: Where is the configured namespace convention stored? DynamoDB config table? SSM Parameter Store? SAM template parameters?
3. **Trust stack discovery**: How does the control plane discover which application accounts have the trust stack deployed? A registration API? A DynamoDB table of registered accounts?
4. **Approval UI**: The human-in-the-loop approval needs a user interface or at minimum an API + CLI. What does the MVP approval experience look like?
5. **Subscription management architecture**: This change covers domain management only. Subscription management (subscribe/unsubscribe, subscription rules, bus-to-bus routing) will need its own architecture change.
