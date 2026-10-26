# Register and manage domains

[Documentation home](../README.md)

For domain owners: register a domain, follow approval, read its state, and update metadata. Start with [authentication](authentication.md).

## Overview

Konductor creates event buses, schema registries, dead-letter queues (DLQs), and Amazon CloudWatch dashboards across the configured environments.

You access Konductor through a REST API with two authentication options:

- **AWS Identity and Access Management (IAM)** – Use AWS Signature Version 4 (SigV4) requests for automation and platform tooling.
- **OAuth** – Use an Amazon Cognito token for interactive access.

Both API surfaces support domain registration, domain reads, publishing, subscriptions, and schemas. Platform administration is OAuth-only. This includes registration decisions (`/domains/registrations/decide`), the pending-registrations inbox, binding and entitlement administration, and enabling or disabling domains.

OAuth responses also enforce ownership scope. Team callers see domains their teams own. Platform roles see all domains.

The examples in this guide use `awscurl` with the IAM surface. On the OAuth surface, send the same requests with `curl` and an `Authorization: Bearer` access token (see [calling the API](authentication.md#calling-the-api)), and add `owningTeam` when you register. Approval is a platform-team step on the OAuth surface; as a domain owner you follow its outcome but never call it.

Your domain name is your domain's identity. It is the sole identifier used in API paths, DynamoDB records, event payloads, and resource names. Choose carefully—domain names are globally unique and cannot be renamed.

---

## Registering a domain

To register a domain, send a POST request to `/domains`. Konductor validates the body, sends the command to Amazon Simple Queue Service (Amazon SQS), and returns a request ID. A handler then persists the registration and starts the approval workflow asynchronously.

### Request

This IAM-signed request omits `owningTeam`, which only the OAuth surface accepts. For the OAuth form, see the [curl example](authentication.md#using-curl).

```bash
awscurl -X POST "${API_URL}/domains" \
  --region "${AWS_REGION}" \
  -H "Content-Type: application/json" \
  -d '{
    "domainName": "billing",
    "ownerAccountId": "111122223333",
    "ownerEmail": "billing-team@example.com",
    "secondaryOwnerEmails": ["billing-backup@example.com"],
    "description": "Billing domain events",
    "costCenter": "cc-1234"
  }'
```

### Fields

The API rejects fields that are not in this table with `400`.

| Field | Required | Description |
|-------|----------|-------------|
| `domainName` | Yes | Globally unique domain name. Must match `^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$` (lowercase alphanumeric with hyphens, 2-64 characters). Cannot start or end with a hyphen. |
| `ownerAccountId` | Yes | The 12-digit AWS account ID where your domain's application-side resources will be provisioned by default. The trust stack must already be deployed in this account. |
| `environmentAccounts` | No | Sparse map of environment name to 12-digit AWS account ID, pinning individual environments to accounts other than `ownerAccountId`. Keys must be members of the platform's configured environment list; values need not be unique. The trust stack must already be deployed in every distinct account you name. Fixed at registration time. |
| `ownerEmail` | Yes | Email address for the domain owner. Receives every owner notification (see [What you will receive](#what-you-will-receive)). |
| `secondaryOwnerEmails` | Yes | One or more secondary owner email addresses. At least one is required. Secondary owners are backup contacts for when the primary owner is unavailable, and receive the same owner notifications. |
| `owningTeam` | OAuth: Yes. IAM: not accepted | The Cognito team group that owns the domain after approval. OAuth callers must name one of their own team groups; platform groups are never team groups. An OAuth registration without a matching `owningTeam` is refused with `RegisterDomainFailed` and no domain record is created. The IAM surface rejects the field with `400`. A domain registered through IAM gets no owner binding at approval, so a platform administrator must [grant one](../operations/access-administration.md#bootstrapping-role-bindings) before a team can read or update it. |
| `description` | No | Human-readable description (max 256 characters). |
| `costCenter` | No | Cost-allocation value (1–256 characters, no control characters) applied as the `cost-center` tag on every provisioned resource. It is the only custom tag the API accepts; a `tags` map is rejected. You can change it later with an [update](#updating-domain-metadata). |

### Response

```json
{
  "requestId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "message": "Request accepted"
}
```

The `requestId` is the SQS message ID. You can use it to correlate with CloudWatch logs if needed, but it is not the domain identifier—your domain name is.

### What happens next

Registration is asynchronous. The `requestId` confirms only that Konductor accepted the request—the domain does not exist yet. One of three things follows:

- **Accepted for approval.** The domain is recorded as `PENDING_APPROVAL` and sent to the platform team to decide. The environments it will provision are fixed now, from the platform's current environment list (for example, `dev`, `staging`, `prod`); later platform changes do not affect it.
- **Refused.** The request fails a check and never reaches approval. The checks are the `owningTeam` authorization, the naming rules (including reserved names like `konductor`), `costCenter`, `environmentAccounts`, a name already in use (see [Duplicate name handling](#duplicate-name-handling)), and the deployment's [domain ceiling](../operations/service-quotas.md#the-1000-domain-ceiling). Konductor emits `RegisterDomainFailed` and changes no record. If no record holds the name, `GET /domains/{domainName}` keeps returning `404`. If one does, such as a live domain or an earlier `REJECTED` registration, it stays as it was: its `createdAt` predates your request, and its status is not this request's outcome.
- **Rejected by the collision check.** If a resource name the domain needs is already taken (an environment's platform event bus or the `/namespace/{domainName}` parameter), Konductor records the domain as `REJECTED` with `decidedBy: system`. The `justification` names the colliding resources. Ask your platform team to remove them, then register again under the same name.

When notifications are enabled, you also receive a [registration failure email](#what-you-will-receive) with the reason. Without notifications, a refusal leaves nothing to read: check the request against the rules above, or ask your platform administrator.

How you watch the outcome depends on the API surface:

- **IAM:** poll `GET /domains/{domainName}`. It returns the domain as soon as Konductor records it, as `PENDING_APPROVAL` or `REJECTED`.
- **OAuth:** `GET /domains/{domainName}` returns `404` until the domain is approved, because approval creates your team's owner binding. Until then, poll `GET /domains`, which also lists your team's `PENDING_APPROVAL` and `REJECTED` registrations (matched by `owningTeam`). List items omit the `justification`. For the reason behind a rejection, read the outcome email or ask your platform administrator.

Once a platform administrator approves the domain, it moves `PENDING_APPROVAL → APPROVED → PROVISIONING → ACTIVE`, and `GET /domains/{domainName}` returns it on both surfaces; see [Approval process](#approval-process) below. For how registration, approval, and provisioning work internally, see [domain lifecycle](../architecture/domain-lifecycle.md).

### Duplicate name handling

If a live record (any status except `REJECTED`) already holds the name, the transaction's name condition fails and Konductor emits `RegisterDomainFailed` with the reason `domain 'billing' already exists`. A `REJECTED` record does not block the name; see [Rejected email](#rejected-email). The domain name is globally unique across the entire Konductor installation—this is intentional governance, not a technical limitation.

---

## Approval process

After you register a domain, the platform team must approve it before any infrastructure is created. You do not interact with the approval endpoint directly—that is the platform team's responsibility.

Platform administrators decide from **Approvals** in the Konductor console (the **Pending approvals** page), or on the OAuth surface by reading the request's task token from `GET /domains/registrations/pending` and sending it to `POST /domains/registrations/decide` (see the [curl example](authentication.md#using-curl)). A rejection requires a justification. Neither path is available to IAM callers.

### What you will receive

Konductor sends these emails only when notifications are enabled in the [platform configuration](../operations/configure-platform.md#settings-you-can-change-later). Each one goes to `ownerEmail` and every `secondaryOwnerEmails` address.

| Email | When | Subject |
|-------|------|---------|
| Approval outcome (approved) | Platform team approves your domain | `Domain Approved: 'billing'` |
| Approval outcome (rejected) | Platform team rejects your domain, or the approval request expires after 7 days | `Domain Registration Rejected: 'billing'` |
| Registration failure | Konductor refuses or rejects the request before approval (duplicate name, naming rule violation, unauthorized `owningTeam`, invalid `costCenter` or `environmentAccounts`, resource-name collision, or the domain ceiling) | `Domain Registration Failed: 'billing'` |
| Registration delayed | The approval request could not be delivered to the platform team (status `APPROVAL_NOTIFICATION_FAILED`) | `Domain Registration Delayed: 'billing'` |
| Domain live | Provisioning completed and the domain is `ACTIVE` | `Domain Live: 'billing'` |
| Provisioning failure | Provisioning fails after approval | `Domain Provisioning Issue: 'billing'` |

The approval request (`Approval Required: Domain 'billing'`) goes to the platform operator address, not to you. It includes your domain name, owner account ID, owner email, and the environments that will be provisioned, with the account each environment resolves to when the request carried `environmentAccounts` overrides. If no `ACTIVE` domain uses your owner account yet, the email includes a trust-stack deployment advisory for the platform team. The email links to **Pending approvals** in the console; it contains no approve or reject links.

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
> | Rejected By | `a1b2c3d4-e5f6-7890-abcd-ef1234567890` |
> | Justification | Name conflicts with existing bounded context |

**Rejected By** is the deciding administrator's Cognito user ID (the access token's `sub` claim), or `system:timeout` when the approval request expired.

A rejected domain can be re-registered under the same name once the issue is corrected. Submit a new registration request with the corrected details and the rejected record is atomically replaced with a fresh `PENDING_APPROVAL` record (new `requestId`, fresh approval cycle). The rejection history remains available through `DomainRegistrationRejected` events on the control plane bus.

### Approval timeout

The approval workflow waits up to 7 days for a decision. If none arrives, Konductor rejects the registration: the status becomes `REJECTED`, `decidedBy` is `system:timeout`, the justification reads `Approval request expired after 7 days without a decision`, and you receive the rejection email. Register again to start a fresh approval cycle, and ask your platform team to decide before the new request expires. See [domain recovery](../operations/domain-recovery.md#domain-stuck-in-pending_approval) for registrations that stay pending.

---

## Checking domain status

Retrieve details about a specific domain using its domain name:

```bash
awscurl -X GET "${API_URL}/domains/billing" --region "${AWS_REGION}"
```

### Response (ACTIVE domain)

This example is abbreviated: `resourceArns` holds one entry for each environment in `environments`, and only `dev` is shown.

```json
{
  "domainName": "billing",
  "namespace": "acme-billing",
  "description": "Billing domain events",
  "ownerAccountId": "111122223333",
  "ownerEmail": "billing-team@example.com",
  "secondaryOwnerEmails": ["billing-backup@example.com"],
  "costCenter": "cc-1234",
  "status": "ACTIVE",
  "environments": ["dev", "staging", "prod"],
  "environmentAccounts": {
    "dev": "111122223333",
    "staging": "111122223333",
    "prod": "111122223333"
  },
  "tags": {
    "konductor:managed-by": "konductor",
    "konductor:domain": "billing",
    "konductor:namespace": "acme-billing",
    "cost-center": "cc-1234"
  },
  "resourceArns": {
    "dev": {
      "platformEventBusArn": "arn:aws:events:ap-southeast-2:444455556666:event-bus/acme-dev-billing-event-bus",
      "appEventBusArn": "arn:aws:events:ap-southeast-2:111122223333:event-bus/acme-dev-billing-event-bus",
      "schemaRegistryArn": "arn:aws:schemas:ap-southeast-2:444455556666:registry/acme-dev-billing-schema-registry",
      "subscriptionDlqArn": "arn:aws:sqs:ap-southeast-2:444455556666:acme-dev-billing-subscription-dlq",
      "integrationDlqArn": "arn:aws:sqs:ap-southeast-2:111122223333:acme-dev-billing-integration-dlq",
      "dashboardArn": "arn:aws:cloudwatch::444455556666:dashboard/acme-dev-billing-dashboard"
    }
  },
  "namespaceParameter": "/namespace/billing",
  "createdAt": "2026-06-26T10:30:00.000Z",
  "updatedAt": "2026-06-26T10:31:15.000Z",
  "decidedBy": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "justification": "",
  "approvedAt": "2026-06-26T10:30:45.000Z",
  "systemEventsTopicArn": "arn:aws:sns:ap-southeast-2:444455556666:acme-billing-system-events"
}
```

Platform-side resources (the platform event bus, schema registry, subscription DLQ, and dashboard) live in the Konductor platform account, `444455556666` in this example. The application-side event bus and integration DLQ live in each environment's account from `environmentAccounts`.

### Field reference

| Field | Description |
|-------|-------------|
| `domainName` | The domain's unique name (partition key). |
| `namespace` | Environment-independent namespace identity, `{org}-{domainName}`. |
| `description` | Human-readable description. |
| `ownerAccountId` | AWS account where app-side resources are provisioned by default. |
| `environmentAccounts` | Effective environment → account map (overrides applied). Absent on responses from stacks predating the field. |
| `ownerEmail` | Domain owner email. |
| `secondaryOwnerEmails` | Secondary owner email addresses. |
| `costCenter` | The governed cost-allocation value, or `null` when none is set. |
| `status` | Current lifecycle status (see [Domain Lifecycle](../reference/domain-statuses.md)). |
| `environments` | Environments this domain is provisioned into. Snapshotted at registration time. |
| `tags` | The platform-governed tag set Konductor applies to the domain's resources. Read-only; change `costCenter` to change the `cost-center` tag. |
| `resourceArns` | Amazon Resource Names (ARNs) for each environment. Populated only when status is `ACTIVE`. Each key is an environment name. |
| `namespaceParameter` | AWS Systems Manager (SSM) parameter path for the environment-independent domain namespace. |
| `createdAt` | Registration timestamp. |
| `updatedAt` | Last update timestamp. |
| `decidedBy` | Who decided the registration: the platform administrator's Cognito user ID (`sub`), `system:timeout` when approval expired, or `system` when the resource-name collision check rejected the request before approval. |
| `justification` | Approver or rejecter justification. |
| `approvedAt` | Approval timestamp (if approved). |
| `rejectedAt` | Rejection timestamp (if rejected). |
| `errorDetail` | Failure detail for a failed approval notification or provisioning run, when present. |
| `systemEventsTopicArn` | The domain's [system events](system-events.md) topic, once provisioning has recorded it. |

### Response (non-ACTIVE domain)

When the domain is not yet `ACTIVE`, `resourceArns` is `null` and `approvedAt` might not be set. For `PENDING_APPROVAL` domains, the response includes the same metadata fields but no resource information.

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
awscurl -X GET "${API_URL}/domains" --region "${AWS_REGION}"

# Filter by owner account
awscurl -X GET "${API_URL}/domains?ownerAccountId=111122223333" --region "${AWS_REGION}"

# Filter by status
awscurl -X GET "${API_URL}/domains?status=ACTIVE" --region "${AWS_REGION}"

# Paginate
awscurl -X GET "${API_URL}/domains?nextToken=<token>&maxResults=50" --region "${AWS_REGION}"
```

### Response (list)

```json
{
  "domains": [
    {
      "domainName": "billing",
      "domainNamespace": "acme-billing",
      "description": "Billing domain events",
      "ownerAccountId": "111122223333",
      "ownerEmail": "billing-team@example.com",
      "secondaryOwnerEmails": ["billing-backup@example.com"],
      "status": "ACTIVE",
      "createdAt": "2026-06-26T10:30:00.000Z"
    },
    {
      "domainName": "sales",
      "domainNamespace": "acme-sales",
      "description": "Sales domain events",
      "ownerAccountId": "111122223333",
      "ownerEmail": "sales-team@example.com",
      "secondaryOwnerEmails": ["sales-backup@example.com"],
      "status": "PROVISIONING",
      "createdAt": "2026-06-26T11:00:00.000Z"
    }
  ],
  "nextToken": "eyJ2ZXJzaW9uIjoxLCJsYXN0S2V5..."
}
```

List items are summaries. Use `GET /domains/{domainName}` for environments, environment accounts, and resource ARNs.

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
  --region "${AWS_REGION}" \
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

### Response (update)

```json
{
  "requestId": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
  "message": "Request accepted"
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

When `costCenter` changes, the `UpdateDomainSucceeded` event (with `costCenter` in `changedFields`) also triggers the **retag workflow** (`konductor-retag-domain`). The workflow converges the `cost-center` tag to the new value on every provisioned resource: both event buses, schema registries, DLQs, dashboards, and alarms across all environments, plus the namespace parameter. Retagging is asynchronous and idempotent: tag calls are upserts, so a redelivered event that starts a duplicate execution converges to the same tag state. A retag failure emits `RetagDomainFailed` and never reverts the committed record change.

## Domain lifecycle and resources

See [domain statuses](../reference/domain-statuses.md) for owner actions and the environment snapshot, and [resources and naming](../reference/resources.md) for what provisioning creates.

## Next steps

- [Publish events](publishing-events.md)
- [Subscribe to events](subscriptions.md)
- [Troubleshoot or clean up a domain](../operations/domain-recovery.md)
