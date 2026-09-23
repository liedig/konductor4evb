# Domain recovery and cleanup

[Documentation home](../README.md)

For platform operators investigating domain lifecycle failures. The commands below change live state only when an operator separately authorizes them.

> **Review pending:** [D02](../reference/documentation-discrepancies.md#d02), [D10](../reference/documentation-discrepancies.md#d10) records conflicting or unverified guidance retained from the previous documentation. This reorganization does not resolve it.

## Troubleshooting

### Domain stuck in PENDING_APPROVAL

The approval workflow waits up to 7 days. If your domain has been pending longer than expected:

1. Check the domain status via `GET /domains/{domainName}`.
2. Contact your platform team—the approval email may be sitting unread.
3. If the 7-day timeout passes, the workflow marks the domain `REJECTED` (`decidedBy: system:timeout`) and you receive the standard rejection notification. Re-register when ready.

### APPROVAL_NOTIFICATION_FAILED

The registration was accepted but the approval request email could not be delivered (most commonly the platform's SES sender identity is unverified or SES is misconfigured). **Nobody rejected your domain**—no decision was ever made, and no owner email is sent (email delivery is exactly what failed).

Recovery is operator-driven:

1. The platform team fixes the notification transport (for example, verifies the SES identity—see `notifications-stack.yaml`).
2. They delete the failed record and you re-register, which restarts the approval flow.

Platform operators can find these domains with:

```bash
awscurl --service execute-api "https://{api-id}.execute-api.{region}.amazonaws.com/prod/domains?status=APPROVAL_NOTIFICATION_FAILED"
```

Operators should also sweep for records left in `PROVISIONING` by workflow crashes predating the failure-path hardening (2026-07): `?status=PROVISIONING` with no running `konductor-provision-domain` execution means a stuck record—delete it and re-register.

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

If you try to register a domain name that already exists in a status other than `REJECTED`, you will see a `RegisterDomainFailed` event with reason "domain already exists" and receive a failure notification email at the submitted `ownerEmail`. Domain names are globally unique while in use. Choose a different name, or if the existing domain is `REJECTED`, simply correct your submission and re-register—the rejected record is replaced atomically.

### API returns 403

On the IAM surface, ensure your AWS credentials have permission to invoke the API Gateway, and verify that your `awscurl --region` matches the Region where Konductor is deployed. On the OAuth surface, check that your access token is unexpired and carries the role the route requires (for example, `platform-admin` for approval and binding administration). For publish and schema writes specifically, a `403` means your principal lacks a `publisher` binding for the domain—see [Publishing Events](../guides/publishing-events.md#before-you-can-publish-publisher-bindings).

## Cleaning up domain resources

A `DELETE /domains` API endpoint is not yet available. To remove a domain and its resources, a platform operator can use the teardown script, which drives the same deprovision-env workflow used by the provisioning saga's rollback path:

```bash
make teardown-domain DOMAIN_NAME=<domainName>
```

This removes all per-environment resources (event buses, schema registries, DLQs, dashboards, alarms) in both the platform and domain accounts and deletes the domain's DynamoDB record. The SSM namespace parameter and RAM resource share association are not removed by the script and must be cleaned up manually if needed.

If you need to re-register the same domain name after cleanup, ensure all stale resources are removed first to avoid `ResourceAlreadyExistsException` during provisioning.
