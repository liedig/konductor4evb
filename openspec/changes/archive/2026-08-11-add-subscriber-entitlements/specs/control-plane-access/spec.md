## ADDED Requirements

### Requirement: Entitlement administration is OAuth-surface only

Entitlement administration routes (`POST /entitlements`, `GET /entitlements`, `DELETE /entitlements/{entitlementId}`) SHALL exist only on the OAuth control-plane surface, wired to the Konductor REQUEST authorizer. The IAM surface SHALL NOT expose entitlement administration in this capability version. Administration authorization SHALL combine the `platform-admin` global role, resolved by the OAuth authorizer from controlled Cognito groups independently of grant storage, with resource-role bindings evaluated by the grant engine: a `publisher` binding on the entitlement's producer domain authorizes create and revoke for that producer domain, and a `subscriber` binding authorizes listing the bound consumer domain's entitlements. Subscription resource routes and the consumer schema catalog SHALL remain available on both surfaces with identical post-normalization authorization semantics.

#### Scenario: Entitlement routes reject IAM-surface access

- **WHEN** any caller requests an entitlement administration route on the IAM control-plane surface
- **THEN** no such route SHALL exist

#### Scenario: Producer self-service uses normalized bindings

- **WHEN** an OAuth caller with a `publisher` binding on producer domain `orders` administers entitlements covering `orders`
- **THEN** the OAuth surface SHALL authorize the operation through the grant engine without requiring the `platform-admin` role

#### Scenario: Subscription resource routes remain dual-surface

- **WHEN** a caller invokes subscription create, state, delete, list, or detail routes, or the consumer schema catalog
- **THEN** those routes SHALL exist on both the OAuth and IAM surfaces
- **AND THEN** both SHALL enforce the same engine decision after trusted-principal normalization
