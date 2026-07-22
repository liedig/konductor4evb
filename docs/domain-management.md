# Domain Management Guide

This guide is for **domain owners** — teams who want to register a new EventBridge domain, get it approved, and start using the provisioned infrastructure. It covers the full lifecycle from registration through active operation.

If you are a platform team member responsible for approving registrations or managing the Nexus installation itself, see the [Installation Guide](installation.md) for deployment and the [Trust Stack Deployment Guide](trust-stack-deployment.md) for onboarding domain accounts.

---

## Overview

EventBridge Nexus provisions and manages the cross-account EventBridge infrastructure your domain needs: event buses, schema registries, DLQs, and CloudWatch dashboards across all configured environments. You interact with Nexus through a REST API with IAM authentication.

Your domain name is your domain's identity. It is the sole identifier used in API paths, DynamoDB records, event payloads, and resource names. Choose carefully — domain names are globally unique and cannot be renamed.

---

## Registering a Domain

Register a domain by sending a POST request to the `/domains` endpoint. The request is asynchronous: Nexus validates the body, enqueues the command to SQS, and returns a request ID immediately. The actual processing (persistence, approval workflow trigger) happens moments later.

### Request

```bash
awscurl -X POST "${API_URL}/domains" \
  --region "${REGION}" \
  -H "Content-Type: application/json" \
  -d '{
    "domainName": "billing",
    "ownerAccountId": "111122223333",
    "ownerEmail": "billing-team@example.com",
    "secondaryOwnerEmails": ["billing-backup@example.com"],
    "description": "Billing domain events",
    "tags": {
      "team": "finance",
      "cost-center": "cc-1234"
    }
  }'
```

### Fields

| Field | Required | Description |
|-------|----------|-------------|
| `domainName` | Yes | Globally unique domain name. Must match `^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$` (lowercase alphanumeric with hyphens, 2-64 characters). Cannot start or end with a hyphen. |
| `ownerAccountId` | Yes | The 12-digit AWS account ID where your domain's application-side resources will be provisioned. The trust stack must already be deployed in this account. |
| `ownerEmail` | Yes | Email address for the domain owner. Receives approval and rejection outcome notifications. |
| `secondaryOwnerEmails` | Yes | One or more secondary owner email addresses. At least one is required. Secondary owners are backup contacts for when the primary owner is unavailable. |
| `description` | No | Human-readable description (max 256 characters). |
| `tags` | No | Key-value string map for organisational metadata. |

### Response

```json
{
  "requestId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "message": "Request accepted"
}
```

The `requestId` is the SQS message ID. You can use it to correlate with CloudWatch logs if needed, but it is not the domain identifier — your domain name is.

### What happens next

1. The command handler Lambda processes the message from SQS.
2. It validates business rules (naming format, reserved names like `nexus`).
3. It performs a conditional `PutItem` against the domains table with `attribute_not_exists(domainName)` — if the name is already taken, the registration fails with a "domain already exists" error event.
4. It stamps the current platform-configured environment list (e.g., `["dev", "staging", "prod"]`) onto the domain record. This list is snapshotted at registration time and does not change if the platform configuration is updated later.
5. It starts the approval Step Functions workflow.
6. The platform team receives an approval request email with one-click approve/reject links.

### Duplicate name handling

If a domain with the same name already exists, the conditional PutItem fails and a `RegisterDomainFailed` event is emitted with reason "domain already exists". The domain name is globally unique across the entire Nexus installation — this is intentional governance, not a technical limitation.

---

## Approval Process

After you register a domain, the platform team must approve it before any infrastructure is created. You do not interact with the approval endpoint directly — that is the platform team's responsibility.

### What you will receive

| Email | When | Sent to | Subject |
|-------|------|---------|---------|
| Approval outcome (approved) | Platform team approves your domain | `ownerEmail` | `Domain Approved: 'billing'` |
| Approval outcome (rejected) | Platform team rejects your domain | `ownerEmail` | `Domain Registration Rejected: 'billing'` |
| Registration failure | Registration fails deterministic validation (duplicate name, naming rule violation) | `ownerEmail` | `Domain Registration Failed: 'billing'` |
| Provisioning failure | Provisioning fails after approval | `ownerEmail` | `Domain Provisioning Issue: 'billing'` |

The approval request email goes to the platform team inbox, not to you. It includes your domain name, owner account ID, owner email, and the environments that will be provisioned. If this is the first domain registered from your AWS account, the email includes a trust-stack deployment advisory for the platform team.

### Approved email

When your domain is approved, you receive an email confirming the approval and listing the environments that will be provisioned:

> **Domain Registration Approved**
>
> Your domain **billing** has been approved.
>
> Provisioning is now underway across the following environments: **dev, staging, prod**.
>
> You will receive a follow-up notification if provisioning encounters any issues.

Provisioning typically completes within 1-2 minutes. You can poll `GET /domains/{domainName}` to check the status transition from `PROVISIONING` to `ACTIVE`.

### Rejected email

If your domain is rejected, you receive an email with the rejecter's identity and justification:

> **Domain Registration Rejected**
>
> | Field | Value |
> |-------|-------|
> | Domain Name | billing |
> | Owner Account | 111122223333 |
> | Rejected By | admin@example.com |
> | Justification | Name conflicts with existing bounded context |

A rejected domain can be re-registered under the same name once the issue is corrected. Submit a new registration request with the corrected details and the rejected record is atomically replaced with a fresh `PENDING_APPROVAL` record (new `requestId`, fresh approval cycle). The rejection history remains available through `DomainRegistrationRejected` events on the control plane bus.

### Approval timeout

The approval workflow waits up to 7 days for a decision. If no decision is received, the workflow times out and the domain remains in `PENDING_APPROVAL`. Contact your platform team to resolve stale approvals.

---

## Checking Domain Status

Retrieve details about a specific domain using its domain name:

```bash
awscurl -X GET "${API_URL}/domains/billing" --region "${REGION}"
```

### Response (ACTIVE domain)

```json
{
  "domainName": "billing",
  "description": "Billing domain events",
  "ownerAccountId": "111122223333",
  "ownerEmail": "billing-team@example.com",
  "secondaryOwnerEmails": ["billing-backup@example.com"],
  "status": "ACTIVE",
  "environments": ["dev", "staging", "prod"],
  "tags": {
    "team": "finance"
  },
  "resourceArns": {
    "dev": {
      "platformEventBusArn": "arn:aws:events:ap-southeast-2:111122223333:event-bus/acme-dev-billing-event-bus",
      "appEventBusArn": "arn:aws:events:ap-southeast-2:444455556666:event-bus/acme-dev-billing-event-bus",
      "schemaRegistryArn": "arn:aws:schemas:ap-southeast-2:111122223333:registry/acme-dev-billing-schema-registry",
      "subscriptionDlqArn": "arn:aws:sqs:ap-southeast-2:111122223333:acme-dev-billing-subscription-dlq",
      "integrationDlqArn": "arn:aws:sqs:ap-southeast-2:444455556666:acme-dev-billing-integration-dlq",
      "dashboardArn": "arn:aws:cloudwatch::111122223333:dashboard/acme-dev-billing-dashboard"
    },
    "staging": { "..." },
    "prod": { "..." }
  },
  "namespaceParameter": "/namespace/billing",
  "createdAt": "2026-06-26T10:30:00.000Z",
  "updatedAt": "2026-06-26T10:31:15.000Z",
  "decidedBy": "admin@example.com",
  "justification": "",
  "approvedAt": "2026-06-26T10:30:45.000Z"
}
```

### Field reference

| Field | Description |
|-------|-------------|
| `domainName` | The domain's unique name (partition key). |
| `description` | Human-readable description. |
| `ownerAccountId` | AWS account where app-side resources are provisioned. |
| `ownerEmail` | Domain owner email. |
| `secondaryOwnerEmails` | Secondary owner email addresses. |
| `status` | Current lifecycle status (see [Domain Lifecycle](#domain-lifecycle)). |
| `environments` | Environments this domain is provisioned into. Snapshotted at registration time. |
| `tags` | Key-value metadata. |
| `resourceArns` | Per-environment resource ARNs. Only populated when status is `ACTIVE`. Each key is an environment name. |
| `namespaceParameter` | SSM parameter path for the domain namespace (env-independent). |
| `createdAt` | Registration timestamp. |
| `updatedAt` | Last update timestamp. |
| `decidedBy` | Who approved or rejected the domain. |
| `justification` | Approver or rejecter justification. |
| `approvedAt` | Approval timestamp (if approved). |

### Response (non-ACTIVE domain)

When the domain is not yet `ACTIVE`, `resourceArns` is absent and `approvedAt` may not be set. For `PENDING_APPROVAL` domains, the response includes the same metadata fields but no resource information.

### Not found

If the domain name does not exist, the API returns `404` with:

```json
{
  "code": "NOT_FOUND",
  "message": "domain not found: billing"
}
```

---

## Listing Domains

List all domains you have access to, with optional filters:

```bash
# List all domains
awscurl -X GET "${API_URL}/domains" --region "${REGION}"

# Filter by owner account
awscurl -X GET "${API_URL}/domains?ownerAccountId=111122223333" --region "${REGION}"

# Filter by status
awscurl -X GET "${API_URL}/domains?status=ACTIVE" --region "${REGION}"

# Paginate
awscurl -X GET "${API_URL}/domains?nextToken=<token>&maxResults=50" --region "${REGION}"
```

### Response

```json
{
  "items": [
    {
      "domainName": "billing",
      "description": "Billing domain events",
      "ownerAccountId": "111122223333",
      "status": "ACTIVE",
      "environments": ["dev", "staging", "prod"],
      "createdAt": "2026-06-26T10:30:00.000Z"
    },
    {
      "domainName": "sales",
      "description": "Sales domain events",
      "ownerAccountId": "111122223333",
      "status": "PROVISIONING",
      "environments": ["dev", "staging", "prod"],
      "createdAt": "2026-06-26T11:00:00.000Z"
    }
  ],
  "nextToken": "eyJ2ZXJzaW9uIjoxLCJsYXN0S2V5..."
}
```

### Query parameters

| Parameter | Description | Default |
|-----------|-------------|---------|
| `ownerAccountId` | Filter by owner AWS account ID. | None (all domains) |
| `status` | Filter by lifecycle status. | None (all statuses) |
| `nextToken` | Pagination token from a previous response. | None |
| `maxResults` | Maximum items per page. | 100 (max 100) |

---

## Updating Domain Metadata

Update mutable metadata for an existing domain. This is asynchronous — the API accepts the command and returns a request ID. The outcome is communicated via EventBridge events.

```bash
awscurl -X PUT "${API_URL}/domains/billing" \
  --region "${REGION}" \
  -H "Content-Type: application/json" \
  -d '{
    "description": "Billing and invoicing domain events",
    "tags": {
      "team": "finance",
      "cost-center": "cc-5678"
    }
  }'
```

### Mutable fields

| Field | Mutable | Notes |
|-------|---------|-------|
| `description` | Yes | Max 256 characters. |
| `tags` | Yes | Key-value string map. Replaces the entire tags map. |
| `domainName` | No | Cannot be renamed. Register a new domain instead. |
| `status` | No | Status transitions are managed by workflows, not the update API. |
| `ownerAccountId` | No | Fixed at registration time. |
| `ownerEmail` | Yes | Mutable via update endpoint. |
| `secondaryOwnerEmails` | Yes | One or more emails. Replaces the entire set. |

### Response

```json
{
  "requestId": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
  "message": "Request accepted"
}
```

---

## Domain Lifecycle

Your domain moves through a well-defined status progression:

```
                 POST /domains
                      │
                      ▼
              PENDING_APPROVAL
                      │
          ┌───────────┴───────────┐
          │                       │
       Approve                 Reject
          │                       │
          ▼                       ▼
      PROVISIONING            REJECTED
          │                  (terminal)
          │
    ┌─────┴──────┐
    │            │
    ▼            ▼
  ACTIVE   PROVISIONING_FAILED
```

| Status | Meaning | What to do |
|--------|---------|-------------|
| `PENDING_APPROVAL` | Registration submitted, awaiting platform team decision | Wait for approval email. Contact platform team if it takes longer than expected. |
| `APPROVED` | Approved, provisioning not yet started | Nothing — this is a transient state. You should see `PROVISIONING` within seconds. |
| `REJECTED` | Registration denied | Review the rejection email for justification. Correct the submission and re-register under the same name. |
| `PROVISIONING` | Infrastructure creation in progress across all environments | Wait. Typically completes within 1-2 minutes. Poll `GET /domains/{domainName}` to check. |
| `ACTIVE` | All resources created, domain is operational | Start using your event buses. See [Provisioned Resources](#provisioned-resources). |
| `PROVISIONING_FAILED` | Resource creation failed, rollback completed | Check the provisioning failure email. Contact your platform team — they can inspect Step Functions execution history for details. |

### Environment snapshot

The environments your domain is provisioned into are determined by the platform configuration at registration time, not by your request. Every approved domain is provisioned into the full platform-configured list (default: `dev`, `staging`, `prod`). This list is snapshotted onto your domain record at registration time and does not change if the platform team adds or removes environments later.

---

## Provisioned Resources

When your domain reaches `ACTIVE`, Nexus has created resources in two AWS accounts across all your environments:

### Platform account resources (per environment)

| Resource | Name pattern | Purpose |
|----------|-------------|---------|
| Event bus | `{org}-{env}-{domain}-event-bus` | Inbound event bus for your domain |
| PutEvents policy | (attached to event bus) | Allows your owner account to publish events |
| Schema registry | `{org}-{env}-{domain}-schema-registry` | EventBridge schema registry for discovery |
| Subscription DLQ | `{org}-{env}-{domain}-subscription-dlq` | Dead-letter queue for failed subscription deliveries |
| CloudWatch dashboard | `{org}-{env}-{domain}-dashboard` | Operational monitoring dashboard |
| Failed invocations alarm | `{org}-{env}-{domain}-failed-invocations-alarm` | Alarm for failed event invocations |
| DLQ depth alarm | `{org}-{env}-{domain}-dlq-depth-alarm` | Alarm for DLQ message accumulation |

### Domain account resources (per environment)

| Resource | Name pattern | Purpose |
|----------|-------------|---------|
| Event bus | `{org}-{env}-{domain}-event-bus` | Your domain's event bus in your account |
| Integration DLQ | `{org}-{env}-{domain}-integration-dlq` | Dead-letter queue for failed integrations |

### Domain-level resources (environment-independent)

| Resource | Name | Purpose |
|----------|------|---------|
| SSM parameter | `/namespace/{domain}` | Namespace identifier shared across the organization via RAM |
| RAM resource share | `{org}-domain-namespaces` | Organization-wide sharing of domain namespace parameters |

### Naming convention example

For org `acme`, domain `billing`, environment `dev`:

```
acme-dev-billing-event-bus          (platform account)
acme-dev-billing-schema-registry    (platform account)
acme-dev-billing-subscription-dlq   (platform account)
acme-dev-billing-dashboard          (platform account)
acme-dev-billing-event-bus          (domain account)
acme-dev-billing-integration-dlq    (domain account)
/namespace/billing                  (SSM parameter, shared via RAM)
```

The platform-side and app-side event buses share the same name — they live in different accounts, so there is no collision.

---

## Event Subscriptions

Once domains are `ACTIVE`, teams connect them with **schema-first subscriptions**: you subscribe to an event contract (a schema in the producer's registry), and Nexus builds the delivery stream in every environment. You never touch EventBridge rules or cross-account policies on the platform side.

### Discovering schemas

Browse what a producer domain offers:

```bash
awscurl -X GET "${API_URL}/schemas?domain=orders" --region ${AWS_REGION}
```

The response lists each schema (named `{source}@{event-name}`, e.g. `acme-orders@OrderNotification`) with the **latest version per environment** — so you can see before subscribing if dev is ahead of prod.

### Subscribing

```bash
awscurl -X POST "${API_URL}/subscriptions" \
  --region ${AWS_REGION} \
  -H "Content-Type: application/json" \
  -d '{
    "consumerDomain": "checkout",
    "producerDomain": "orders",
    "schemaName": "acme-orders@OrderNotification"
  }'
```

Key things to know:

- **You always subscribe to the latest schema version** — it is resolved and pinned per environment when the subscription activates. There is no version parameter: the schema is the contract, and there is one current representation of an event.
- **The caller's AWS account must own the consumer domain.** Subscribing is a consumer-side action.
- **Activation is all-or-nothing across environments.** If the schema is missing in any environment's registry, the subscription fails as a whole.
- **Start with an environment paused** (e.g., avoid production event volume while developing) by passing initial state: `"environments": {"prod": {"enabled": false}}`.

Track progress with `GET /subscriptions/{consumerDomain}.{producerDomain}.{eventName}` — status moves `PENDING → ACTIVE` (or `FAILED` with error detail).

### Consuming events

Once the stream is `ACTIVE`, matching events arrive on **your app-side domain bus** (`{org}-{env}-{consumerDomain}-event-bus`). Processing them is your responsibility: create your own EventBridge rules on your bus targeting your Lambda functions, queues, or state machines. This is the only direct interaction your team has with an event bus — the platform owns everything upstream.

### Pausing and resuming

```bash
awscurl -X PATCH "${API_URL}/subscriptions/checkout.orders.OrderNotification" \
  --region ${AWS_REGION} \
  -H "Content-Type: application/json" \
  -d '{"environments": {"prod": {"enabled": false}}}'
```

Disabling flips the delivery rule off atomically — the subscription, its pinned version, and the contract remain intact. Use it for cost control during development or as a kill switch during incidents.

### Unsubscribing

```bash
awscurl -X DELETE "${API_URL}/subscriptions/checkout.orders.OrderNotification" --region ${AWS_REGION}
```

Nexus removes the platform-side delivery rules in every environment; the subscription record is retained for 90 days for audit. **Any integration rules you created on your own bus are yours to clean up** — the platform never modifies your bus.

### Producer obligations: the event envelope

Until the Publish API ships, producers put events onto their platform-side bus directly and **must stamp the canonical envelope** — subscription filtering depends on it:

```json
{
  "Source": "acme-orders",
  "DetailType": "OrderNotification",
  "Detail": {
    "metadata": { "schemaVersion": 3 },
    "data": { "orderId": "..." }
  }
}
```

- `Source` is your domain's namespace identity (`{org}-{domain}`) — the same value in every environment.
- `DetailType` is the event name, matching the schema name's suffix after `@`.
- `detail.metadata.schemaVersion` must be **numeric** and match a registered schema version. Events with a missing or string-typed version match no subscriptions — they silently deliver to no one.

Register your event schemas in your per-environment schema registries (named `{source}@{event-name}`) via the AWS Schemas API; the registry access policies created at provisioning permit your owner account to write.

---

## Troubleshooting

### Domain stuck in PENDING_APPROVAL

The approval workflow waits up to 7 days. If your domain has been pending longer than expected:

1. Check the domain status via `GET /domains/{domainName}`.
2. Contact your platform team — the approval email may be sitting unread.
3. If the 7-day timeout passes, the workflow marks the domain `REJECTED` (`decidedBy: system:timeout`) and you receive the standard rejection notification. Re-register when ready.

### APPROVAL_NOTIFICATION_FAILED

The registration was accepted but the approval request email could not be delivered (most commonly the platform's SES sender identity is unverified or SES is misconfigured). **Nobody rejected your domain** — no decision was ever made, and no owner email is sent (email delivery is exactly what failed).

Recovery is operator-driven:

1. The platform team fixes the notification transport (e.g., verifies the SES identity — see `notifications-stack.yaml`).
2. They delete the failed record and you re-register, which restarts the approval flow.

Platform operators can find these domains with:

```bash
awscurl --service execute-api "https://{api-id}.execute-api.{region}.amazonaws.com/v1/domains?status=APPROVAL_NOTIFICATION_FAILED"
```

Operators should also sweep for records left in `PROVISIONING` by workflow crashes predating the failure-path hardening (2026-07): `?status=PROVISIONING` with no running `nexus-provision-domain` execution means a stuck record — delete it and re-register.

### PROVISIONING_FAILED

If provisioning fails, you receive a vague notification email:

> **Domain Provisioning Issue**
>
> Your domain **billing** encountered a problem during provisioning.
>
> The platform team has been notified and will follow up. No action is required from you at this time.

The email intentionally does not include technical details. Your platform team can investigate via Step Functions execution history. Common causes:

- **Trust role not deployed**: The trust stack must be deployed in your owner account before approval. If this is the first domain from your account, the approval email included a trust-stack advisory.
- **Resource name collision**: Stale resources from a previous registration attempt may still exist. Your platform team can clean these up.
- **IAM permission issues**: The provisioning workflow role may lack permissions. This is a platform team concern.

### Duplicate registration error

If you try to register a domain name that already exists in a status other than `REJECTED`, you will see a `RegisterDomainFailed` event with reason "domain already exists" and receive a failure notification email at the submitted `ownerEmail`. Domain names are globally unique while in use. Choose a different name, or if the existing domain is `REJECTED`, simply correct your submission and re-register — the rejected record is replaced atomically.

### API returns 403

The Nexus API uses IAM authentication. Ensure your AWS credentials have permission to invoke the API Gateway. If you are using `awscurl`, verify that your `--region` matches the region where Nexus is deployed.

### Subscription active but no events arriving

1. Confirm the producer is publishing with the canonical envelope — especially a **numeric** `detail.metadata.schemaVersion`. A string-typed version matches no rules.
2. Compare the version the producer stamps with your pinned version: `GET /subscriptions/{subscriptionId}` shows `pinnedVersion` per environment. If the producer moved to a newer version after you subscribed, your rule no longer matches — this is contract enforcement, not a bug. Coordinate with the producer team.
3. Check the environment is enabled (`enabled: true` in the per-env map).
4. Check the per-subscription zero-events alarm and the producer's subscription DLQ for delivery failures.

### Subscribe fails with a validation error

`SubscribeFailed` outcome events carry the reason: both domains must be `ACTIVE`, your account must own the consumer domain, the schema's `source` must match the producer's namespace identity, and the derived rule name (`{consumerDomain}-{eventName}`) must fit EventBridge's 64-character/`[A-Za-z0-9._-]` constraints.

---

## Clean Up

A `DELETE /domains` API endpoint is not yet available. To remove a domain and its resources, a platform operator can use the teardown script, which drives the same deprovision-env workflow used by the provisioning saga's rollback path:

```bash
make teardown-domain DOMAIN_NAME=<domainName>
```

This removes all per-environment resources (event buses, schema registries, DLQs, dashboards, alarms) in both the platform and domain accounts and deletes the domain's DynamoDB record. The SSM namespace parameter and RAM resource share association are not removed by the script and must be cleaned up manually if needed.

If you need to re-register the same domain name after cleanup, ensure all stale resources are removed first to avoid `ResourceAlreadyExistsException` during provisioning.
