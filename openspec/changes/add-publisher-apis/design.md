## Context

Schema discovery is currently read-only through Nexus, while domain owners register schemas out-of-band. Event publication likewise has no control-plane API that can enforce canonical identity and version stamping. The access-foundation change introduces separate OAuth and IAM control-plane surfaces plus normalized trusted principals. The authorization-engine spike validates the selected DynamoDB-backed grant engine but explicitly does not implement it. This change therefore owns the production publisher-binding administration, persistence, and decision adapter for that engine.

The write path differs from existing asynchronous control-plane commands. Schema writes must return the registry result, and publication must resolve the latest schema version and observe the `PutEvents` entry outcome in the same request. These operations therefore require synchronous Lambda integrations.

## Goals / Non-Goals

**Goals:**

- Give Cognito team groups and AWS accounts with a domain `publisher` binding a synchronous, domain- and environment-scoped API for schema creation, append-only version registration, and event publication.
- Expose equivalent publisher routes on the separate OAuth and IAM control-plane surfaces, normalize each principal, and enforce one resource-decision contract.
- Give OAuth `platform-admin` callers production administration of persistent publisher-to-domain bindings through the DynamoDB-backed grant engine.
- Keep canonical names, envelope fields, registry selection, bus selection, and latest-version selection platform-owned.
- Preserve least privilege and make domain authorization occur before target resource access.
- Return deterministic API errors for authorization, missing resources, conflicts, validation failures, and downstream AWS failures.

**Non-Goals:**

- Schema promotion between environments, including promotion policy, approval, or version selection.
- Business-payload validation against JSON Schema or OpenAPI schema content.
- Schema deletion, retirement, historical mutation, or registry administration.
- Management of buses, policies, rules, subscriber grants, non-publisher roles, or authorization configuration beyond publisher-binding administration.
- A general event-ingress Lambda or replacement for API Gateway-to-SQS ingestion when asynchronous durability and buffering are required.
- Subscriber catalog or entitlement workflows.

## Decisions

### Use explicit domain/environment resource routes

The APIs will be:

- `POST /domains/{domain}/environments/{environment}/schemas`
- `POST /domains/{domain}/environments/{environment}/schemas/{eventName}/versions`
- `POST /domains/{domain}/environments/{environment}/events/{eventName}`

Each publisher route is exposed with the same contract on the separate OAuth and IAM control-plane surfaces. Schema creation accepts a schema document; version append accepts the next schema document; publish accepts only `data`. API Gateway request models reject unsupported identity, version, envelope, registry, and bus fields. Shared handlers consume the foundation's normalized principal contract, so route behavior after authentication is mechanism-neutral.

Putting domain and environment in the resource path makes the authorization resource and deployment target unambiguous. A query-based extension to the existing discovery API was considered, but it obscures the target resource and would mix discovery semantics with publisher-only mutation. A single route accepting either OAuth or IAM credentials was rejected because it would violate the access foundation's separate-surface contract.

### Deliver platform-admin publisher-binding administration

The OAuth platform-administration surface will expose:

- `POST /domains/{domain}/publisher-bindings`
- `GET /domains/{domain}/publisher-bindings`
- `DELETE /domains/{domain}/publisher-bindings/{bindingId}`

Only a normalized OAuth principal with the global `platform-admin` role can invoke these operations. A create request selects exactly one typed principal: an issuer-qualified Cognito team group or a twelve-digit AWS account. One record binds that principal to role `publisher` on one domain; assigning one principal to several domains creates distinct bindings. The API does not expose a global publisher binding.

The production DynamoDB authorization adapter validated by `evaluate-authorization-engine` durably persists binding identifier, role, domain, typed principal, creation metadata, and trusted actor. Creates are idempotent for the same typed principal, role, and domain; conflicting or malformed requests fail without partial state. List is platform-admin-only. Revoke removes the identified binding, invalidates any decision cache required by the selected design, and preserves access supplied by an overlapping binding. Every mutation writes a token-free audit record.

Treating Cognito groups as global role groups was rejected because the same team can publish for one domain and not another. Storing IAM principals only as exact role ARNs was also rejected for this slice because settled publisher administration requires AWS-account principals and retained IAM callers must share the same domain semantics.

### Authorize normalized OAuth and IAM principals before AWS lookups

OAuth handlers receive issuer-qualified subject and trusted Cognito team-group claims from the access-foundation authorizer context. IAM handlers receive the trusted AWS account and IAM principal identity from API Gateway normalization. Both submit the same closed action, resource type `domain`, and target domain to one production authorization interface.

The DynamoDB grant engine matches OAuth principals through issuer-qualified team-group bindings and IAM principals through AWS-account bindings. It preserves principal type and provenance so textual identifiers cannot cross-match. One binding covers the bound domain in all supported environments, but the environment remains explicit per request.

Handlers deny by default on missing context, no matching binding, or authorization-engine failure. They perform the authorization check before reading domain, registry, or schema data so unauthorized callers cannot probe target-domain state. A global Cognito `publisher` group or claim was considered and rejected because publisher is a Nexus resource role scoped by bindings, not an identity-system role.

### Resolve all resource identity from active domain state

After authorization, handlers read the domain record, require `ACTIVE`, validate that the requested environment is supported, and obtain the registered namespace identity. They derive:

- schema name: `{namespaceIdentity}@{eventName}`
- registry: `{org}-{environment}-{domain}-schema-registry`
- event source: `{namespaceIdentity}`
- detail type: `{eventName}`
- target platform bus: the selected environment's domain bus

Callers cannot supply any of these values. Central derivation avoids drift and prevents publishing to arbitrary platform resources. Accepting caller-supplied ARNs or canonical names was rejected because it would broaden both the attack surface and IAM scope.

### Use focused synchronous Lambda handlers

Schema creation, version append, and event publishing use focused Lambda proxy integrations shared by equivalent OAuth and IAM routes after principal normalization. The schema handlers call AWS Schemas `CreateSchema` and version-producing `UpdateSchema` operations and translate duplicate or missing targets into `409` or `404`. No delete or generic update operation is exposed.

The publish handler is a dedicated synchronous Lambda. It calls AWS Schemas for the derived schema's latest version, builds and validates one canonical EventBridge entry, calls `PutEvents`, and inspects both request-level errors and per-entry errors. It returns `202 Accepted` only when EventBridge accepts the entry.

Direct API Gateway service integrations were considered. They cannot perform the required authorization decision, domain lookup, latest-version resolution, envelope assembly, and per-entry outcome translation as one coherent operation. Routing publication through the existing command queue was also rejected because the caller would not receive immediate schema-resolution or EventBridge acceptance results.

### Treat schema versions as immutable registry snapshots

Creating a schema establishes version 1; appending content creates the next registry version atomically. The API never accepts a version number for write targeting and never calls delete or historical mutation operations. Concurrent append requests rely on AWS Schemas to allocate distinct versions, and each response returns the version created by its operation.

The latest version used for a publish is the version observed during that request. A concurrent append after resolution does not invalidate an accepted event stamped with the prior version. Client-selected versions were considered and rejected because this capability intentionally defines publication against current selected-environment state.

### Construct and validate the envelope without validating business data

The publish handler constructs:

```json
{
  "source": "{namespaceIdentity}",
  "detail-type": "{eventName}",
  "detail": {
    "metadata": {
      "schemaVersion": 4
    },
    "data": {}
  }
}
```

Validation covers event-name constraints, required envelope structure, numeric positive schema version, JSON serialization, and EventBridge entry limits. The schema document is not fetched to validate `data`; only latest schema metadata is needed. Payload validation was considered but rejected for this change because it adds latency, schema-language concerns, and compatibility semantics not established by the current contract.

### Constrain infrastructure permissions to publisher operations

The publisher operation handler roles receive only domain-state read access, authorization-decision access, and the AWS operation set required by their route. Schema roles can create schemas or append versions in Nexus-managed domain registries but cannot manage registry lifecycle or policies. The publish role can describe schemas and publish only to Nexus domain-bus resource patterns; it cannot manage buses, rules, or policies. Separate binding-administration roles can call only the grant engine's binding create/list/revoke operations and audit sink, and their routes require OAuth `platform-admin`.

Provisioning and existing access policies will stop granting application teams direct registry administration or generic schema mutation. Platform administration retains registry lifecycle ownership. Application-level authorization remains mandatory because dynamic domain bindings are narrower than deploy-time IAM resource patterns.

## Risks / Trade-offs

- [The authorization-engine spike validates but does not implement the production engine] → Make its completed readiness decision an implementation prerequisite, then build and test the production publisher binding adapter, persistence, administration, and decision path in this change.
- [Two publisher route surfaces can drift] → Generate or verify equivalent route contracts from shared schemas and run OAuth/IAM parity tests against the same handlers and decision fixtures.
- [Binding revocation can be hidden by stale decision caches] → Use the grant engine's proven invalidation design, make successful revoke mean durable acceptance, and test subsequent current-state denial plus overlapping-binding behavior.
- [AWS Schemas metadata can be briefly eventually consistent after a write] → Return the concrete created version from schema writes and document that publish resolves the latest version visible at request time; map transient AWS failures without falsely accepting an event.
- [Latest-version resolution adds latency to every publish] → Keep the first implementation uncached for correctness; emit latency metrics so a bounded, invalidatable cache can be evaluated separately.
- [A synchronous publish response confirms EventBridge acceptance, not downstream delivery] → Use `202 Accepted` and avoid delivery-success language in the response contract.
- [Lambda IAM patterns cover many dynamic domain resources] → Combine resource-pattern IAM with mandatory resource-role authorization and tests proving cross-domain denial occurs before AWS access.
- [Removing direct registry writes can disrupt existing out-of-band workflows] → Deploy APIs and publisher bindings first, verify usage, then tighten direct access policies with rollback templates retained.

## Migration Plan

1. Complete `evaluate-authorization-engine` and implement the DynamoDB grant engine's production publisher-binding persistence, adapter, audit path, and platform-admin administration routes.
2. Add equivalent publisher route contracts to the OAuth and IAM surfaces, shared models and handlers, envelope/schema helpers, and least-privilege roles.
3. Create initial Cognito team-group and AWS-account bindings through the platform-admin API and verify cross-surface decision parity and revocation.
4. Deploy schema creation and append handlers, then the dedicated publish handler.
5. Exercise domain isolation, schema append, latest-version stamping, EventBridge failure behavior, and OAuth/IAM parity in a non-production environment.
6. Remove direct application-team registry administration and generic schema-write access from provisioning policies and update existing managed policies after API readiness is confirmed.
7. Roll back publisher routes and operation permissions without deleting binding audit history. If the grant engine adapter must be disabled, fail publisher decisions closed; restore the prior direct schema-write policy only if operationally required. Schemas and versions created through the APIs remain valid and require no data rollback.

## Open Questions

None. Schema promotion remains intentionally deferred rather than unresolved within this change.
