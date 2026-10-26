# Subscribe to events

[Documentation home](../README.md)

For consumer teams connecting ACTIVE domains. Have [API credentials](authentication.md), a subscriber binding, and entitlement coverage before starting.

## Event subscriptions

Once domains are `ACTIVE`, teams connect them with **schema-first subscriptions**: you subscribe to an event contract (a schema in the producer's registry), and Konductor builds the delivery stream in every environment. You never touch EventBridge rules or cross-account policies on the platform side.

### Discovering schemas

Your catalog shows exactly what your domain may subscribe to—nothing more:

```bash
awscurl -X GET "${API_URL}/schemas?consumerDomain=checkout" --region "${AWS_REGION}"
```

The response expands your domain's **entitlements** (granted by the platform team) against producer registries. It lists each covered schema (named `{source}@{event-name}`, for example, `acme-orders@OrderNotification`) with the **latest version per environment**, so you can see before subscribing if dev is ahead of prod. If the catalog is empty, your domain has no entitlements yet: ask the platform team to grant one (for example, `orders` / `OrderNotification*`). Filter with `&domain=orders` or `&environment=prod`, and page with `nextToken` (up to 50 entries per page).

Entitlements follow one grammar: `*` covers a producer's whole domain (future schemas included), an exact event name covers all its versions (future versions included), and a trailing `*` covers a name family. You never write patterns yourself—the platform grants them—but reading your catalog tells you exactly which grammar you received.

### Subscribing

```bash
awscurl -X POST "${API_URL}/subscriptions" \
  --region "${AWS_REGION}" \
  -H "Content-Type: application/json" \
  -d '{
    "consumerDomain": "checkout",
    "producerDomain": "orders",
    "schemaName": "acme-orders@OrderNotification"
  }'
```

Key things to know:

- **You always subscribe to the latest schema version**—it is resolved and pinned per environment when the subscription activates. There is no version parameter: the schema is the contract, and there is one current representation of an event.
- **You need a subscriber binding and entitlement coverage.** Your principal must hold the `subscriber` role on the consumer domain (granted via the binding administration API), and an entitlement must cover the target schema. Both are checked at subscribe time—and again inside the activation workflow, so a grant revoked mid-flight stops before any rule is created. A `403` on an existing subscription or schema means a missing binding or coverage, not a missing resource.
- **Each environment activates on its own.** The subscription becomes `ACTIVE` as soon as one environment materializes. An environment whose registry does not have the schema yet waits as `AWAITING_PROMOTION` and activates automatically when the producer promotes the schema there. The subscription fails as a whole only when no environment can materialize, which usually means a mistyped schema name.
- **Start with an environment paused** (for example, avoid production event volume while developing) by passing initial state: `"environments": {"prod": {"enabled": false}}`.

Track progress with `GET /subscriptions/{consumerDomain}.{producerDomain}.{eventName}`—status moves `PENDING → ACTIVE`, or to `FAILED` with error detail when no environment materializes. Each environment entry carries its own status (`ACTIVE`, `AWAITING_PROMOTION`, or `FAILED`). List your domain's streams with `GET /subscriptions?domain=checkout&direction=inbound` (or `direction=outbound` as a producer).

How you retry depends on the subscription's overall status. In an `ACTIVE` subscription, re-enable a `FAILED` environment with `POST /subscriptions/state` to retry it. A subscription that is `FAILED` as a whole accepts no state changes: correct the cause, then send `POST /subscriptions` again, which starts a new activation.

### Consuming events

Once the stream is `ACTIVE`, matching events arrive on **your app-side domain bus** (`{org}-{env}-{consumerDomain}-event-bus`). Processing them is your responsibility: create your own EventBridge rules on your bus targeting your Lambda functions, queues, or state machines. This is the only direct interaction your team has with an event bus—the platform owns everything upstream.

### Pausing and resuming

```bash
awscurl -X POST "${API_URL}/subscriptions/state" \
  --region "${AWS_REGION}" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "checkout.orders.OrderNotification",
    "environments": {"prod": {"enabled": false}}
  }'
```

Disabling flips the delivery rule off atomically—the subscription, its pinned version, and the contract remain intact. Use it for cost control during development or as a kill switch during incidents. Re-enabling rechecks entitlement coverage: if the platform revoked your entitlement while the stream was paused, the enable is denied. Revocation itself never deletes your subscription—it disables the delivery rules, and a new grant plus an explicit re-enable restores the stream.

### Unsubscribing

```bash
awscurl -X DELETE "${API_URL}/subscriptions/checkout.orders.OrderNotification" --region "${AWS_REGION}"
```

Konductor removes the platform-side delivery rules in every environment; the subscription record is retained for 90 days for audit. **Any integration rules you created on your own bus are yours to clean up**—the platform never modifies your bus.

## Event contract

Read the [canonical envelope](../reference/event-envelope.md) and [publishing guide](publishing-events.md) with the producer. The [materialization explanation](../architecture/subscriptions.md) covers per-environment state.

## Troubleshooting

### Subscription active but no events arriving

1. Confirm the producer's events carry the canonical envelope—especially a **numeric** `detail.metadata.schemaVersion`. Events published through the Publish API are stamped correctly by construction; this check matters for producers publishing directly to their bus, where a string-typed version matches no rules.
2. Compare the version the producer stamps with your pinned version: `GET /subscriptions/{subscriptionId}` shows `pinnedVersion` per environment. If the producer moved to a newer version after you subscribed, your rule no longer matches—this is contract enforcement, not a bug. Coordinate with the producer team.
3. Check the environment is enabled (`enabled: true` in the per-env map) and its entry is `ACTIVE`. An `AWAITING_PROMOTION` environment has no delivery rule until the producer promotes the schema there.
4. Check the per-subscription zero-events alarm and the producer's subscription DLQ for delivery failures.

### Subscribe fails with a validation error

`SubscribeFailed` events include the failure reason. Check the following requirements:

- Both domains must be `ACTIVE`.
- You must hold a `subscriber` binding on the consumer domain and a current entitlement for the producer schema. Revoked entitlements also prevent re-enabling a subscription.
- The schema's `source` must match the producer's namespace identity.
- The derived rule name (`{consumerDomain}-{eventName}`) must use `[A-Za-z0-9._-]` and fit the 64-character limit for EventBridge.

---

<a id="clean-up"></a>
