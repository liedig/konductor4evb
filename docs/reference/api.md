# API contracts

[Documentation home](../README.md)

The OpenAPI files are the maintained route and request/response inventories:

- [OAuth API](../../api/openapi-oauth.yaml), for interactive callers.
- [IAM API](../../api/openapi-iam.yaml), for AWS-signed automation.

Use [authentication](../guides/authentication.md) to call them, and the
[application-team guides](../guides/README.md) for task examples. API integration
and authorization boundaries are explained in [identity and authorization](../architecture/authorization.md).

Do not add another endpoint table to a guide or the architecture overview.
Update the applicable OpenAPI file and capability requirement when a route
changes. Import the contract into an API client rather than copying an old table.

## Requirements and evidence

[OpenSpec capability specs](../../openspec/specs/) describe required behavior.
Handlers and tests provide implementation evidence. A disagreement between
them requires review; it is not resolved by selecting whichever source is newest.

The previous Markdown endpoint tables remain in the repository only as
[dated review evidence](../history/documentation-baseline.md). Their role
requirements predate binding-based authorization; use
[identity and authorization](../architecture/authorization.md) for current access rules.
