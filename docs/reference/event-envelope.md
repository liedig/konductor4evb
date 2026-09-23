# Canonical event envelope

[Documentation home](../README.md)

The business-event identity and metadata that subscription rules match. Use the [publishing guide](../guides/publishing-events.md) for producer requests.

## The canonical event envelope

Every event flowing through Konductor domain buses carries a canonical identity, and subscription rules filter on it:

- `source`—the producing domain's env-independent namespace identity `{org}-{domain}` (for example, `acme-orders`), derived by the platform, never free-typed
- `detail-type`—the event name
- Schema name—`{source}@{detail-type}` (the native EventBridge discovery convention; round-trips to `aws schemas describe-schema`)
- Schema version—**numeric**, stamped at `detail.metadata.schemaVersion` (`detail` splits into `metadata`/`data`)
- Idempotency key—`detail.metadata.idempotencyKey`, caller-supplied or the API Gateway request ID
- Publish time—`detail.metadata.publishedAt` (ISO-8601 milliseconds), derived from the SQS `SentTimestamp` so retries never move it

An event a producer publishes—every field here is a scalar, exactly as `PutEvents` expects:

```json
{
  "Source": "acme-orders",
  "DetailType": "OrderNotification",
  "Detail": "{\"metadata\":{\"schemaVersion\":3,\"idempotencyKey\":\"key-123\",\"publishedAt\":\"2026-07-30T10:30:00.123Z\"},\"data\":{\"orderId\":\"o-123\"}}"
}
```

The subscription rule Konductor materializes to match it—EventBridge event patterns filter with arrays, not scalars, which is why this looks different from the event above despite matching on the same three fields:

```json
{
  "source": ["acme-orders"],
  "detail-type": ["OrderNotification"],
  "detail": { "metadata": { "schemaVersion": [3] } }
}
```

With platform metadata active, the `metadata` block additionally carries the stamped `metadataRevision` and every field the active metadata revision generates (`eventId`, `geo.region`, …)—the platform supplies all metadata values, and caller-supplied injected fields are rejected at admission. The stamped block is exactly the active revision's compiled schema; consumers validating it exactly must tolerate additive revisions (a revision bump is a new contract, not a guarantee that old readers accept new fields):

```json
{
  "Source": "acme-orders",
  "DetailType": "OrderNotification",
  "Detail": "{\"metadata\":{\"schemaVersion\":3,\"metadataRevision\":2,\"idempotencyKey\":\"key-123\",\"publishedAt\":\"2026-07-30T10:30:00.123Z\",\"eventId\":\"0b9e6c21-7f4a-4d8e-9f1b-3c2d5e4f6a7b\",\"geo\":{\"region\":\"ap-southeast-2\"}},\"data\":{\"orderId\":\"o-123\"}}"
}
```

Producers publish through `POST /publish`; Konductor stamps the entire envelope. Direct `PutEvents` to a domain bus remains possible for domain-internal events, but mis-stamped events fail closed (no rule matches—non-delivery, never misdelivery).

> **Review pending:** [D11](../reference/documentation-discrepancies.md#d11) records conflicting or unverified guidance retained from the previous documentation. This reorganization does not resolve it.

## Producer obligations: the event envelope

Producers publish through the **Publish API** (`POST /publish`), which derives the canonical identity, resolves the latest schema version, and stamps the envelope for you—see [Publishing Events](../guides/publishing-events.md). Schema registration is likewise in-platform via `POST /schemas` and `POST /schemas/versions`.

Direct publishing to your platform-side bus remains possible (the provisioning-created PutEvents policy permits your owner account), but then **you** must stamp the canonical envelope—subscription filtering depends on it:

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

- `Source` is your domain's namespace identity (`{org}-{domain}`)—the same value in every environment.
- `DetailType` is the event name, matching the schema name's suffix after `@`.
- `detail.metadata.schemaVersion` must be **numeric** and match a registered schema version. Events with a missing or string-typed version match no subscriptions—they silently deliver to no one.
