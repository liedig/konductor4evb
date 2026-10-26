# Domain recovery and cleanup

[Documentation home](../README.md)

For platform operators investigating domain lifecycle failures. The commands below change live state only when an operator separately authorizes them.

## Troubleshooting

### Domain stuck in PENDING_APPROVAL

The approval workflow waits up to 7 days. If your domain has been pending longer than expected:

1. Check the domain status via `GET /domains/{domainName}`.
2. Contact your platform team. Every pending request appears in the console's **Pending approvals** inbox; the approval email, sent only when notifications are enabled, may be sitting unread.
3. If the 7-day timeout passes, the workflow marks the domain `REJECTED` (`decidedBy: system:timeout`) and you receive the standard rejection notification. Re-register when ready.

If the platform operator never received the approval email, SES may have rejected it permanently, for example because the sender or recipient identity is unverified while the account is in the SES sandbox. Konductor logs a permanent rejection (`MessageRejected` or `BadRequestException`) without retrying and leaves the request pending, so the platform team can still decide it from **Pending approvals**. Verify both SES identities (see `notifications-stack.yaml`) so later requests are delivered.

### APPROVAL_NOTIFICATION_FAILED

The registration was accepted, but the approval workflow's request step failed. Either it could not record the approval task token, or SES returned an error other than a permanent rejection (for example, persistent throttling or a service error). **Nobody rejected your domain**—no decision was ever made. The workflow emits `DomainApprovalNotificationFailed`: platform operators receive it through the `konductor-platform-notifications` topic, and you and any secondary owners receive a **Domain Registration Delayed** email. That email also goes through SES, so it may not arrive when SES itself is the problem; the operator notification uses Amazon SNS and does not depend on SES.

Recovery is operator-driven:

1. The platform team reads the failure in the domain's `errorDetail` (returned by `GET /domains/{domainName}`) and fixes its cause, for example by restoring SES sending.
2. They delete the failed record with the [teardown script](#cleaning-up-domain-resources), and you re-register, which restarts the approval flow.

Platform operators can find these domains with:

```bash
make list-domains DOMAIN_STATUS=APPROVAL_NOTIFICATION_FAILED AWS_REGION=<region>
```

Also check for domains stuck in `PROVISIONING`. Run `make list-domains DOMAIN_STATUS=PROVISIONING AWS_REGION=<region>`. A listed domain with no running `konductor-provision-domain` execution is stuck: remove it with the [teardown script](#cleaning-up-domain-resources), then re-register.

### PROVISIONING_FAILED

If provisioning fails and notifications are enabled, you receive a vague notification email:

> **Domain Provisioning Issue**
>
> Your domain **billing** encountered a problem during provisioning.
>
> The platform team has been notified and will follow up. No action is required from you at this time.

The email intentionally does not include technical details. Your platform team can investigate via Step Functions execution history. Common causes:

- **Trust role not deployed**: The trust stack must be deployed in your owner account before approval. If this is the first domain from your account, the approval email included a trust-stack advisory.
- **Resource name collision**: Stale resources from a previous registration attempt may still exist. Your platform team can clean these up.
- **IAM permission issues**: The provisioning workflow role may lack permissions. This is a platform team concern.

After fixing the cause, a platform administrator retries from **All domains** in the console (**Retry provisioning**) or with `POST /domains/{domainName}/state` and the body `{"action": "retry-provisioning"}`. The domain returns to `APPROVED`, and the provisioning workflow runs again.

### Duplicate registration error

If you register a domain name that already exists in a status other than `REJECTED`, Konductor emits `RegisterDomainFailed` with the reason `domain '<domainName>' already exists`. When notifications are enabled, the submitted `ownerEmail` and secondary owner addresses also receive a failure notification email. Domain names are globally unique while in use. Choose a different name, or if the existing domain is `REJECTED`, simply correct your submission and re-register—the rejected record is replaced atomically.

### API returns 403

On the IAM surface, ensure your AWS credentials have permission to invoke the API Gateway, and verify that your `awscurl --region` matches the Region where Konductor is deployed. On the OAuth surface, check that your access token is unexpired and carries the role the route requires (for example, `platform-admin` for approval and binding administration). For publish and schema writes specifically, a `403` means your principal lacks a `publisher` binding for the domain—see [Publish events](../guides/publishing-events.md#before-you-can-publish-publisher-bindings).

## Cleaning up domain resources

A `DELETE /domains` API endpoint is not yet available. To remove a domain and its resources, a platform operator can use the teardown script, which drives the same deprovision-env workflow used by the provisioning saga's rollback path.

The teardown does not remove subscriptions, so remove the domain's subscriptions in both directions first. An outbound subscription left in place blocks the teardown; see [schemas and event bus rules block deprovisioning](#schemas-and-event-bus-rules-block-deprovisioning). An inbound one outlives the domain: its record stays `ACTIVE`, and its delivery rule on the producer's platform event bus, with the rule's target and zero-events alarm, stays in place. Subscribing again after you re-register the domain does not replace it, because a subscribe request for an `ACTIVE` subscription is a no-op.

1. List the subscriptions with `GET /subscriptions?domain=<domainName>&direction=outbound` (other domains subscribed to its events) and `GET /subscriptions?domain=<domainName>&direction=inbound` (its subscriptions to other domains' events).
2. Remove each one that is not already `REMOVED` with `DELETE /subscriptions/{subscriptionId}`. A platform administrator needs no `subscriber` binding to do this.
3. Wait until `GET /subscriptions/{subscriptionId}` shows each one as `REMOVED`. Removal runs asynchronously and sets that status only after it deletes the subscription's rules and targets in every environment. Deleting its zero-events alarms is best effort: a failed alarm deletion is not retried and does not hold back `REMOVED`.
4. Check every environment for each subscription's zero-events alarm, and delete any that remain. The alarms are in the platform account. Each is named `<org>-<env>-<ruleName>-zero-events-alarm`, where `<ruleName>` is the environment's `ruleName` in `GET /subscriptions/{subscriptionId}` (`<consumerDomain>-<detailType>`). A leftover alarm has no actions, but it treats missing data as breaching, so it stays in the `ALARM` state.

```bash
# A zero-events alarm left behind by subscription removal (platform account)
aws cloudwatch describe-alarms --alarm-names <org>-<env>-<ruleName>-zero-events-alarm \
  --query "MetricAlarms[*].AlarmName" --output text --region <region>
aws cloudwatch delete-alarms --alarm-names <org>-<env>-<ruleName>-zero-events-alarm --region <region>
```

Then run the teardown:

```bash
make teardown-domain AWS_REGION=<region> DOMAIN_NAME=<domainName>
```

This removes all per-environment resources (event buses, schema registries, DLQs, dashboards, alarms) in both the platform and domain accounts. It then deletes the domain's system-events topic, its `/namespace/{domainName}` SSM parameter, and the domain's DynamoDB record. The script does not modify the `{org}-domain-namespaces` RAM resource share, which serves every domain. Deleting the record does not lower the domain-count counter; see [the 1,000-domain ceiling](service-quotas.md#the-1000-domain-ceiling).

For a domain still in `PENDING_APPROVAL`, the script first stops the domain's approval workflow and confirms that it ended. Left running, the workflow would write the record back as `REJECTED` when it rejects or times out, or reject a domain later re-registered under the same name.

The teardown is safe to re-run. Whenever something is left behind, or the approval workflow does not stop, it keeps the domain record so that a later run can finish.

### Schemas and event bus rules block deprovisioning

The deprovision workflow cannot delete a schema registry that still holds schemas, or an event bus that still has rules. That applies to the platform event bus and to the application event bus in the domain account. For a domain that published schemas, has subscribers, or has rules on its application bus, the script reports `FAILED (resources left behind)` for each affected environment and keeps the record. Clear the leftovers, then re-run the teardown:

1. Remove the domain's remaining subscriptions in both directions, as described in [cleaning up domain resources](#cleaning-up-domain-resources). Unsubscribing deletes their rules and targets in every environment. Delete any rule left by an earlier failure directly: its targets first, then the rule.
2. Delete the schemas in each environment's registry. The API keeps schemas append-only, so use the AWS CLI.
3. Remove the rules on each environment's application event bus. Konductor creates no rules there, so any rule on it belongs to the domain team, for example an integration rule for events it subscribes to. Unsubscribing does not remove these rules. Ask the domain team to delete them, or delete them yourself with credentials for the account the environment resolves to (`environmentAccounts` in `GET /domains/{domainName}`), such as the profile you pass to `make deploy-trust` as `APP_PROFILE`. The application bus has the same name as the platform bus. Delete each rule's targets first, then the rule.

```bash
# Rules on the domain's platform event bus (platform account): remove the targets, then the rule
aws events list-rules --event-bus-name <org>-<env>-<domainName>-event-bus \
  --query "Rules[*].Name" --output text --region <region>
aws events list-targets-by-rule --rule <rule-name> --event-bus-name <org>-<env>-<domainName>-event-bus \
  --query "Targets[*].Id" --output text --region <region>
aws events remove-targets --rule <rule-name> --event-bus-name <org>-<env>-<domainName>-event-bus \
  --ids <target-id> --region <region>
aws events delete-rule --name <rule-name> --event-bus-name <org>-<env>-<domainName>-event-bus --region <region>

# Schemas in the domain's registry
aws schemas list-schemas --registry-name <org>-<env>-<domainName>-schema-registry \
  --query "Schemas[*].SchemaName" --output text --region <region>
aws schemas delete-schema --registry-name <org>-<env>-<domainName>-schema-registry \
  --schema-name <schema-name> --region <region>

# Rules on the domain's application event bus: the same commands with the environment account's profile
aws events list-rules --event-bus-name <org>-<env>-<domainName>-event-bus \
  --query "Rules[*].Name" --output text --region <region> --profile <domain-account-profile>
aws events list-targets-by-rule --rule <rule-name> --event-bus-name <org>-<env>-<domainName>-event-bus \
  --query "Targets[*].Id" --output text --region <region> --profile <domain-account-profile>
aws events remove-targets --rule <rule-name> --event-bus-name <org>-<env>-<domainName>-event-bus \
  --ids <target-id> --region <region> --profile <domain-account-profile>
aws events delete-rule --name <rule-name> --event-bus-name <org>-<env>-<domainName>-event-bus \
  --region <region> --profile <domain-account-profile>
```

### Re-registering a name after cleanup

Registration checks for leftovers before it creates a record. If the platform event bus or the `/namespace/{domainName}` parameter for the name still exists, the request is rejected with reason `resource name collision`. Re-run the teardown until it prints `Domain '<domainName>' fully torn down.` before you re-register.
