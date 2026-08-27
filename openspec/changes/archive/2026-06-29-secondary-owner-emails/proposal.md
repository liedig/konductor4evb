## Summary

Add a required `secondaryOwnerEmails` field to domain registration, capturing one or more secondary owner email addresses. Secondary owners are co-managers of the domain who serve as backup contacts when the primary owner is unavailable or leaves the domain. At this stage, secondary owner emails are captured for informational purposes only — no authorization logic or notification routing changes.

Additionally, fix the smoke test email configuration so the domain owner email is distinct from the platform team email, using the pattern `sliedig+{account}+primary@example.com` for the primary owner and `sliedig+{account}+secondary@example.com` for secondary owners.

## Motivation

The primary/secondary owner model is already documented as a design principle in the architecture and specs, but the data model does not yet capture secondary owner information. Without it, there is no way to identify backup contacts for a domain, creating an operational risk when primary owners are unavailable.

Requiring at least one secondary owner enforces the best practice that no domain should have a single point of contact. This is a governance feature, not a technical convenience.

The smoke test email issue compounds this: when the domain owner email matches the platform team email, the approval flow is indistinguishable from self-approval, masking the real-world separation of concerns.

## Design Decisions

- **`secondaryOwnerEmails` is a required field on registration** — at least one secondary owner email must be provided. Enforced by the OpenAPI schema (minItems: 1) and the Rust model.
- **Both `ownerEmail` and `secondaryOwnerEmails` are mutable** — owners can change over time (people leave, rotate, new members join). Both fields can be updated via `PUT /domains/{domainName}`. Previously `ownerEmail` was immutable after registration; this change makes it mutable.
- **Secondary owner emails are informational only** — no notification emails are sent to secondary owners in this change. The primary `ownerEmail` remains the sole notification target for approval, rejection, and provisioning-failure emails.
- **No DynamoDB schema migration** — DynamoDB is schemaless; new records will include the `secondaryOwnerEmails` attribute. Existing records (smoke-test domain) will not have the field until re-registered.
- **Event payloads include `secondaryOwnerEmails`** — the field is included in `RegisterDomainSucceeded`, `DomainRegistrationApproved`, and `DomainRegistrationRejected` event payloads for audit and downstream visibility.
- **Smoke test uses distinct emails** — primary owner: `sliedig+demo+primary@example.com`, secondary owner: `sliedig+demo+secondary@example.com`, platform team: `sliedig+konductor@example.com` (unchanged).

## Affected Areas

- `domain-management`: Registration request gains required `secondaryOwnerEmails` field. Domain record stores the field. Describe and list responses include it. Update endpoint accepts changes to it. Event payloads include it.
- `system-architecture`: Data model table updated to include `secondaryOwnerEmails`.
- `installation`: Smoke test documentation and Makefile updated to use distinct email addresses.

## Out of Scope

- Secondary owner authorization (access control, approval delegation)
- Notifications sent to secondary owners
- Secondary owner management UI or self-service
- Migration of existing domain records (the smoke-test domain will be re-registered)
