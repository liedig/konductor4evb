# Reference

[Documentation home](../README.md)

Reference pages define exact values: settings, routes, fields, names, and
events. They do not walk through tasks. Start with the
[platform setup path](../operations/README.md) or the
[domain guides](../guides/README.md), and come here when a step points you to
an exact contract.

| Reference | What it defines | When you need it |
|---|---|---|
| [Configuration contract](configuration.md) | Every platform setting, its syntax, default, and whether it can change after setup | Filling in `konductor-platform.toml`, or planning a settings change |
| [API contracts](api.md) | The IAM and OAuth API surfaces and their OpenAPI documents | Building a client, or checking a route's request and response shape |
| [Access-token claims](authentication.md) | The OAuth claims Konductor reads and how Cognito groups map to roles | Debugging a `401` or `403`, or planning team groups |
| [Canonical event envelope](event-envelope.md) | The identity and metadata fields on every delivered business event | Writing a consumer, or tracing an event |
| [Domain statuses](domain-statuses.md) | What each domain status means and which owner actions apply | A domain is not `ACTIVE`, or you need its next step |
| [Resources and naming](resources.md) | Every resource Konductor provisions and its name pattern | Finding a bus, queue, or dashboard, or reviewing IAM scope |
| [Lifecycle events](lifecycle-events.md) | The control-plane outcome events on `konductor-bus` | Automating on registration, provisioning, or update outcomes |
| [Data model](data-model.md) | Storage keys and access patterns for domains and subscriptions | Operating the tables directly, or changing the implementation |
| [Discrepancy register](documentation-discrepancies.md) | Documentation claims that conflict or remain unverified | Before relying on a page that shows a review warning |
