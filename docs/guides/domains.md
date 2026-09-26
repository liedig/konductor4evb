# Register and manage domains

[Documentation home](../README.md)

For domain owners: register a domain, follow approval, read its state, and update metadata. Start with [authentication](authentication.md).

> **Review pending:** [D02](../reference/documentation-discrepancies.md#d02), [D10](../reference/documentation-discrepancies.md#d10), [D11](../reference/documentation-discrepancies.md#d11) records conflicting or unverified guidance retained from the previous documentation. This reorganization does not resolve it.

## Overview

Konductor creates event buses, schema registries, dead-letter queues (DLQs), and Amazon CloudWatch dashboards across the configured environments.

You access Konductor through a REST API with two authentication options:

- **AWS Identity and Access Management (IAM)** – Use AWS Signature Version 4 (SigV4) requests for automation and platform tooling.
- **OAuth** – Use an Amazon Cognito token for interactive access.

Both API surfaces support domain registration, domain reads, publishing, subscriptions, and schemas. Platform administration is OAuth-only. This includes registration decisions (`/domains/registrations/decide`), the pending-registrations inbox, binding and entitlement administration, and enabling or disabling domains.

OAuth responses also enforce ownership scope. Team callers see domains their teams own. Platform roles see all domains. This guide uses `awscurl` with the IAM surface.

Your domain name is your domain's identity. It is the sole identifier used in API paths, DynamoDB records, event payloads, and resource names. Choose carefully—domain names are globally unique and cannot be renamed.

---

## Registering a domain

To register a domain, send a POST request to `/domains`. Konductor validates the body, sends the command to Amazon Simple Queue Service (Amazon SQS), and returns a request ID. A handler then persists the registration and starts the approval workflow asynchronously.

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
| `ownerAccountId` | Yes | The 12-digit AWS account ID where your domain's application-side resources will be provisioned by default. The trust stack must already be deployed in this account. |
| `environmentAccounts` | No | Sparse map of environment name to 12-digit AWS account ID, pinning individual environments to accounts other than `ownerAccountId`. Keys must be members of the platform's configured environment list; values need not be unique. The trust stack must already be deployed in every distinct account you name. Fixed at registration time. |
| `ownerEmail` | Yes | Email address for the domain owner. Receives approval and rejection outcome notifications. |
| `secondaryOwnerEmails` | Yes | One or more secondary owner email addresses. At least one is required. Secondary owners are backup contacts for when the primary owner is unavailable. |
| `description` | No | Human-readable description (max 256 characters). |
| `tags` | No | Key-value string map for organizational metadata. |

### Response

```json
{
  "requestId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "message": "Request accepted"
}
```

The `requestId` is the SQS message ID. You can use it to correlate with CloudWatch logs if needed, but it is not the domain identifier—your domain name is.

### What happens next

1. The command handler Lambda processes the message from SQS.
2. It validates business rules (naming format, reserved names like `konductor`).
3. It performs a conditional `PutItem` against the domains table with `attribute_not_exists(domainName)`—if the name is already taken, the registration fails with a "domain already exists" error event.
4. It stamps the current platform-configured environment list (for example, `["dev", "staging", "prod"]`) onto the domain record. This list is snapshotted at registration time and does not change if the platform configuration is updated later.
5. It starts the approval Step Functions workflow.
6. The platform team receives an approval request email with approval and rejection links.

### Duplicate name handling

If a domain with the same name already exists, the conditional PutItem fails and a `RegisterDomainFailed` event is emitted with reason "domain already exists". The domain name is globally unique across the entire Konductor installation—this is intentional governance, not a technical limitation.

---

## Approval process

After you register a domain, the platform team must approve it before any infrastructure is created. You do not interact with the approval endpoint directly—that is the platform team's responsibility.

### What you will receive

| Email | When | Sent to | Subject |
|-------|------|---------|---------|
| Approval outcome (approved) | Platform team approves your domain | `ownerEmail` | `Domain Approved: 'billing'` |
| Approval outcome (rejected) | Platform team rejects your domain | `ownerEmail` | `Domain Registration Rejected: 'billing'` |
| Registration failure | Registration fails deterministic validation (duplicate name, naming rule violation) | `ownerEmail` | `Domain Registration Failed: 'billing'` |
| Provisioning failure | Provisioning fails after approval | `ownerEmail` | `Domain Provisioning Issue: 'billing'` |

The approval request email goes to the platform team inbox, not to you. It includes your domain name, owner account ID, owner email, and the environments that will be provisioned, with the account each environment resolves to when the request carried `environmentAccounts` overrides. If this is the first domain registered from your AWS account, the email includes a trust-stack deployment advisory for the platform team.

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
> | Rejected By | `admin@example.com` |
> | Justification | Name conflicts with existing bounded context |

A rejected domain can be re-registered under the same name once the issue is corrected. Submit a new registration request with the corrected details and the rejected record is atomically replaced with a fresh `PENDING_APPROVAL` record (new `requestId`, fresh approval cycle). The rejection history remains available through `DomainRegistrationRejected` events on the control plane bus.

### Approval timeout

The approval workflow waits up to 7 days for a decision. If no decision is received, the workflow times out and the domain remains in `PENDING_APPROVAL`. Contact your platform team to resolve stale approvals.

---

## Checking domain status

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
| `ownerAccountId` | AWS account where app-side resources are provisioned by default. |
| `environmentAccounts` | Effective environment → account map (overrides applied). Absent on responses from stacks predating the field. |
| `ownerEmail` | Domain owner email. |
| `secondaryOwnerEmails` | Secondary owner email addresses. |
| `status` | Current lifecycle status (see [Domain Lifecycle](../reference/domain-statuses.md)). |
| `environments` | Environments this domain is provisioned into. Snapshotted at registration time. |
| `tags` | Key-value metadata. |
| `resourceArns` | Amazon Resource Names (ARNs) for each environment. Populated only when status is `ACTIVE`. Each key is an environment name. |
| `namespaceParameter` | AWS Systems Manager (SSM) parameter path for the environment-independent domain namespace. |
| `createdAt` | Registration timestamp. |
| `updatedAt` | Last update timestamp. |
| `decidedBy` | Who approved or rejected the domain. |
| `justification` | Approver or rejecter justification. |
| `approvedAt` | Approval timestamp (if approved). |

### Response (non-ACTIVE domain)

When the domain is not yet `ACTIVE`, `resourceArns` is absent and `approvedAt` might not be set. For `PENDING_APPROVAL` domains, the response includes the same metadata fields but no resource information.

### Not found

If the domain name does not exist, the API returns `404` with:

```json
{
  "code": "NOT_FOUND",
  "message": "domain not found: billing"
}
```

---

## Listing domains

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

## Updating domain metadata

Update mutable metadata for an existing domain. This is asynchronous—the API accepts the command and returns a request ID. The outcome is communicated via EventBridge events.

```bash
awscurl -X PUT "${API_URL}/domains/billing" \
  --region "${REGION}" \
  -H "Content-Type: application/json" \
  -d '{
    "description": "Billing and invoicing domain events",
    "costCenter": "cc-5678"
  }'
```

### Mutable fields

| Field | Mutable | Notes |
|-------|---------|-------|
| `description` | Yes | Max 256 characters. |
| `costCenter` | Yes | Change-only: 1–256 characters, with no control characters. Omit the field to leave it unchanged. An empty value is rejected. Changing it retags every provisioned resource. See [Update outcome events](#update-outcome-events). |
| `domainName` | No | Cannot be renamed. Register a new domain instead. |
| `status` | No | Status transitions are managed by workflows, not the update API. |
| `ownerAccountId` | No | Fixed at registration time. |
| `environmentAccounts` | No | Fixed at registration time; the update API does not accept the field. |
| `ownerEmail` | Yes | Mutable via update endpoint. |
| `secondaryOwnerEmails` | Yes | One or more emails. Replaces the entire set. |

### Response

```json
{
  "requestId": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
  "message": "Domain update request accepted"
}
```

### Update outcome events

A successful update emits `UpdateDomainSucceeded` on the control plane bus (`konductor-bus`, source `konductor.domain-management`). The detail carries the committed record state, the fields that changed, and the accepted command's `requestId`:

```json
{
  "domainName": "billing",
  "updatedAt": "2026-09-01T10:31:15.000Z",
  "status": "ACTIVE",
  "changedFields": ["costCenter"],
  "costCenter": "cc-5678",
  "requestId": "b2c3d4e5-f6a7-8901-bcde-f12345678901"
}
```

`costCenter` is present in the detail only when it changed. A failed update emits `UpdateDomainFailed` with `domainName`, `reason`, and `failedAt`.

When `costCenter` changes, an `UpdateDomainSucceeded` event with `costCenter` in `changedFields` additionally triggers the **retag workflow** (`konductor-retag-domain`), which converges the `cost-center` tag on every provisioned resource—both event buses, schema registries, DLQs, dashboards, and alarms across all environments, plus the namespace parameter—to the new value. Retagging is asynchronous and idempotent: tag calls are upserts, so a redelivered event that starts a duplicate execution converges to the same tag state. A retag failure emits `RetagDomainFailed` and never reverts the committed record change.

## Domain lifecycle and resources

See [domain statuses](../reference/domain-statuses.md) for owner actions and the environment snapshot, and [resources and naming](../reference/resources.md) for what provisioning creates.

## Next steps

- [Publish events](publishing-events.md)
- [Subscribe to events](subscriptions.md)
- [Troubleshoot or clean up a domain](../operations/domain-recovery.md)
