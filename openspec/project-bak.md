# Project Context

## Purpose

Control plane for managing AWS EventBridge infrastructure, routing, and configuration. It provides domain-level APIs that let customers publish and subscribe to events across AWS accounts in minutes, without manually creating EventBridge rules, IAM policies, or cross-account delivery infrastructure.

Primary goals:

- Eliminate cross-account EventBridge complexity for developers and architects
- Reduce time-to-first-event from 1–2 days to under 10 minutes for most use cases
- Cut the number of AWS resources developers must manage per subscription by ~60%
- Give platform teams governance over domains and routing while keeping application teams autonomous

## Tech Stack

- **Backend**: Rust
- **API**: REST
- **Database**: Amazon DynamoDB
- **Infrastructure**: AWS
- **Tooling**: OpenSpec (already configured)

## Project Conventions

### Code Style

- **No code comments** - Write self-explanatory code with clear naming
- **Alphabetically sorted imports** - Maintain consistent import ordering
- **File size limit** - Keep files under 300 lines; split when it improves clarity
- **Naming conventions**: 
  - Use descriptive, intention-revealing names
  - Follow language-specific conventions (e.g., camelCase for JavaScript/TypeScript, PascalCase for Go exports)
- **Formatting**: Use project-standard formatters (e.g., Prettier, gofmt, black)
- **SOLID principles** - Separation of concerns without over-engineering

### Architecture Patterns

- **Spec-driven development** - Use OpenSpec for all feature planning and requirements
- **Minimal complexity** - Start simple, add complexity only when clearly required
- **Single responsibility** - Each module/component has one clear purpose
- **Separation of concerns** - Keep business logic, data access, and presentation separate
 - **Serverless control plane** - Prefer managed AWS services (EventBridge, DynamoDB, Lambda, CloudWatch, CloudTrail, IAM, AWS Organizations) and avoid long‑lived servers where possible
 - **RESTful APIs** - External control plane APIs are modeled as REST operations (e.g., register domain, publish event, subscribe/unsubscribe)
 - **Domain abstraction layer** - Model everything around domains, domain event buses, schemas, and subscriptions instead of raw EventBridge resources
 - **Cross-account automation** - The control plane is responsible for provisioning and maintaining cross-account rules, resource policies, and IAM needed for event delivery

### Testing Strategy

- **Critical paths only** - Write tests for essential functionality, not every function
- **AAA pattern** - Arrange, Act, Assert with comments explaining test intent
- **Test structure**: Learn from existing examples in the codebase
- Focus on integration and behavior over unit test coverage metrics
- Prefer end-to-end and integration tests around key flows defined in the BRD:
  - Domain registration and lifecycle (register, list, describe, update, delete)
  - Event publishing (including schema validation and DLQ behavior)
  - Event subscription lifecycle (subscribe, list, describe, update, unsubscribe)
  - Schema registration and versioning (compatibility, impact analysis, notifications)
  - Cross-account automation (IAM/resource policies, rule creation) and monitoring
- Use lightweight unit tests where they materially reduce risk on complex logic or edge cases

### Git Workflow

- **Conventional Commits** - Follow the Conventional Commit Messages specification
  - Format: `<type>[optional scope]: <description>`
  - Types: `feat`, `fix`, `refactor`, `chore`, `test`, `docs`, etc.
  - Breaking changes: Use `!` after type/scope or `BREAKING CHANGE:` footer
- **Branching**: Simple trunk-based workflow with short‑lived feature branches per change (e.g., `feat-domain-management`, `fix-subscription-metrics`)
- **PR process**: Use `.github/pull_request_template.md` for PR descriptions

**Example commit messages:**
```
feat(api): add event routing configuration endpoint
fix(auth): resolve token expiration race condition
refactor(storage): extract database connection logic
BREAKING CHANGE(api): remove deprecated v1 endpoints
```

## Domain Context

**AWS EventBridge Control Plane** - A control plane that manages the configuration, lifecycle, and operational aspects of Event-Driven Architectures built on Amazon EventBridge, with a strong focus on cross-account scenarios.

**Key concepts** (grounded in the PRFAQ/BRD):

- **Domain**: A logical grouping of related events owned by a team; each domain has its own event buses, schema registry, DLQ, and monitoring
- **Domain event buses**: EventBridge buses provisioned per domain in producer and platform accounts; consumers see domains and events, not individual buses
- **Publish API**: Operation that lets producers send events to a domain without knowing underlying bus topology or cross-account routing details
- **Subscribe/Unsubscribe APIs**: Operations that let consumers connect to or disconnect from domain events; the control plane creates/updates/deletes rules, IAM, and resource policies behind the scenes
- **Schema registry and versioning**: Per-domain registries that store event schemas, track versions, analyze compatibility, and support consumer migration plans
- **Event flow visibility**: Mappings that show which domains produce which events, who consumes them, and cross-domain relationships
- **Platform teams vs. application teams**: Platform teams manage domains and control plane policies; application teams use simple APIs to publish and subscribe

## Important Constraints

Constraints and targets (from BRD/PRFAQ, summarized):
- **Performance**:
  - Time-to-first-event for a new subscription should be under 10 minutes for 90% of use cases (including provisioning and cross-account setup)
  - Domain registration should provision all required resources within 5 minutes for 95% of operations
  - Event publishing should route to the correct domain event bus with p99 latency around 100 ms, plus optional schema validation overhead
- **Scalability**:
  - Design for tens of thousands of domains and billions of events per month across cross-account scenarios
  - Automatically handle EventBridge rule limits per bus by distributing events across domain-specific buses
- **Security and governance**:
  - All cross-account operations must be backed by IAM, resource policies, AWS Organizations integration, and CloudTrail audit logs
  - Fine-grained permission controls for domain registration and event subscription
- **Reliability and failure handling**:
  - Domain-level DLQs for failed events, with at least 30 days of message retention for failure analysis and replay-related needs
  - Health metrics and alarms per domain and per subscription
- **Scope limits**:
  - Initial focus on AWS accounts and cross-account EventBridge scenarios (no on-prem or non-AWS sources in v1)
  - Event replay beyond 30 days, multi-region replication, and workflow design are explicitly deferred to later phases

## External Dependencies

Primary external dependencies (from BRD/PRFAQ):
- **Cloud provider**: AWS
- **Core services**: Amazon EventBridge (event buses, rules), AWS Identity and Access Management (IAM), AWS Organizations
- **Event targets and messaging**: Amazon SNS, Amazon SQS, Amazon Kinesis
- **Data store**: Amazon DynamoDB for control plane state and metadata
- **Observability and audit**: Amazon CloudWatch (metrics, dashboards, alarms), AWS CloudTrail (audit logs), AWS X-Ray (tracing)
- **Other**: CI/CD tooling and repository hosting (e.g., GitHub + pipelines) as chosen by the implementation, following standard AWS/serverless best practices
