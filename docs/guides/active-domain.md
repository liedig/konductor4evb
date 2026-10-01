# What an ACTIVE domain provides

[Documentation home](../README.md)

After approval, Konductor provisions your domain in every configured
environment. Provisioning usually takes one to two minutes. When it succeeds,
the domain status becomes `ACTIVE`. If it fails, Konductor rolls back and the
status becomes `PROVISIONING_FAILED`; see [domain statuses](../reference/domain-statuses.md)
for what each status means and what you can do.

## Confirm the status

Describe your domain and check `status`:

```bash
awscurl -X GET "${API_URL}/domains/billing" --region "${REGION}"
```

An `ACTIVE` response carries `resourceArns`, keyed by environment, and
`environmentAccounts`, the account each environment resolved to. The
[domain guide](domains.md#checking-domain-status) shows a full response and
field reference.

## What Konductor created

Each environment gets its own set of resources, split across two accounts:

- **In the platform account**, Konductor owns your domain's platform event
  bus, schema registry, subscription dead-letter queue, dashboard, and alarms.
  You do not manage these directly; you use them through the API.
- **In your domain account** for that environment, Konductor creates your
  application event bus and integration dead-letter queue. This bus is where
  subscribed events arrive, and where you attach your own rules and targets.

Konductor also creates one environment-independent namespace parameter,
`/namespace/{domain}`, shared across your AWS Organization so other accounts
can discover your domain.

[Resources and naming](../reference/resources.md) lists every resource and
its name pattern. [Provisioning and rollback](../architecture/provisioning.md)
explains how they are created.

## What your team can do now

Approval gave your team an `owner` binding on the domain. With it, your team
can describe the domain and [update its metadata](domains.md#updating-domain-metadata),
such as the description and owner emails.

Publishing and subscribing need separate grants from a platform administrator:

| To | You also need | Details |
|---|---|---|
| Register schemas and publish events | A `publisher` binding on the domain | [Publisher bindings](publishing-events.md#before-you-can-publish-publisher-bindings) |
| Subscribe to another domain's events | A `subscriber` binding on your domain and an entitlement covering the producer's schema | [Subscribe to events](subscriptions.md) |

Administrators grant both through
[access administration](../operations/access-administration.md#bootstrapping-role-bindings).

## Next steps

- [Register a schema and publish your first event](publishing-events.md).
- [Discover schemas and subscribe](subscriptions.md) to another domain.
- Consume subscribed events with your own EventBridge rules on your
  application event bus.
- Watch the domain dashboard and alarms for failed deliveries.
- If something goes wrong, see [domain recovery and cleanup](../operations/domain-recovery.md).
