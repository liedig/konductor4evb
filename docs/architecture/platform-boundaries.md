# Platform boundaries

[Documentation home](../README.md)

Account layout and deployment boundaries. Start with the [architecture overview](README.md); use this page for the detailed topology.

> **Review pending:** [D08](../reference/documentation-discrepancies.md#d08) records conflicting or unverified guidance retained from the previous documentation. This reorganization does not resolve it.

## System overview

Konductor runs in a dedicated **platform account** and provisions domain resources into **domain accounts** via cross-account IAM roles. All control-plane state lives in the platform account; domain accounts only contain the resources their domains need.

```mermaid
graph TB
    subgraph platform["Platform Account"]
        api["API Gateway<br/>(OAuth + IAM surfaces)"]
        cognito["Cognito User Pool<br/>(platform-admin / auditor)"]
        authorizer["OAuth REQUEST<br/>Authorizer Lambda"]
        sqs["SQS Command Queue"]
        handler["Command Handler<br/>Lambda"]
        listfn["List Domains<br/>Lambda"]
        describefn["Describe Domain<br/>Lambda"]
        decidefn["Decide Registration<br/>Lambda"]
        notifyfn["Approval Notification<br/>Lambda"]
        rejectfn["Rejection Notification<br/>Lambda"]
        ddb[("DynamoDB<br/>Domains Table")]
        konductorbus["Konductor Event Bus"]
        approval["Approval Workflow<br/>(Step Functions)"]
        provisioning["Provisioning Workflow<br/>(Step Functions)"]
        ses["SES<br/>Notifications"]
        ssm["SSM Parameters<br/>/namespace/*"]
        ram["RAM Resource Share"]
        domainbus["Domain Bus<br/>(per domain)"]
        registry["Schema Registry<br/>(per domain)"]
        subdlq["Subscription DLQ<br/>(per domain)"]
        dashboard["CloudWatch Dashboard<br/>(per domain)"]
    end

    subgraph app["Domain Account"]
        trustrole["Konductor Trust Role"]
        appbus["Domain Bus<br/>(per domain)"]
        intdlq["Integration DLQ<br/>(per domain)"]
    end

    api -->|"writes (POST/PUT)"| sqs
    api -->|"reads (GET)"| listfn
    api -->|"reads (GET)"| describefn
    api -->|"sync (POST)"| decidefn
    sqs --> handler
    handler --> ddb
    handler --> konductorbus
    handler --> approval
    decidefn -->|"SendTaskSuccess/Failure"| approval
    approval -->|"DomainRegistrationApproved"| konductorbus
    approval --> notifyfn
    konductorbus -->|"DomainRegistrationRejected"| rejectfn
    konductorbus -->|"DomainRegistrationApproved"| provisioning
    notifyfn --> ses
    rejectfn --> ses
    provisioning -->|"creates resources"| domainbus
    provisioning -->|"creates resources"| registry
    provisioning -->|"creates resources"| subdlq
    provisioning -->|"creates resources"| dashboard
    provisioning -->|"STS AssumeRole"| trustrole
    trustrole -->|"creates resources"| appbus
    trustrole -->|"creates resources"| intdlq
    provisioning --> ssm
    ssm --> ram
    listfn --> ddb
    describefn --> ddb
```

### Key design decisions

The platform uses the following design boundaries:

- **Three-stack deployment model**: `notifications-stack.yaml` (optional, SES identities), `konductor-core-stack.yaml` (core platform resources, with conditional notification resources), and `trust-stack.yaml` (per domain account). A fourth, separately authorized package—`guardrail-stack.yaml` (organization-admin SCPs)—exists outside the ordinary deployment path; see [Organization Guardrails](../operations/guardrails.md).
- **Konductor is a singleton per org**—one deployment of `konductor-core-stack.yaml` per AWS organization. Platform resources use the fixed `konductor-*` naming (no org or env segment). The domain-account trust role is the one exception (`{org}-konductor-trust-role`, org-scoped for cross-org uniqueness). Domain resources use `{org}-{env}-{domain}-*` and are provisioned per environment from the platform-configured `Environments` list.
- **One Lambda per API operation**—small, focused functions with least-privilege IAM
- **Reads are synchronous** (API Gateway -> Lambda -> DynamoDB); writes are asynchronous (API Gateway -> SQS -> Lambda -> EventBridge)
- **Step Functions for workflows**—no intermediate Lambdas for AWS API calls; uses SDK integrations with JSONata

For the rationale, see the [Decision log](../decisions/README.md).
