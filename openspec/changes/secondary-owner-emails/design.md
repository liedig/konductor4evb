## Context

The domain record currently has a single `ownerEmail` field representing the primary owner. The primary/secondary owner model is documented as a design principle but not implemented in the data model. This change adds `secondaryOwnerEmails` as a required field on the `RegisterDomainRequest`, persists it on the domain record, and includes it in API responses and event payloads.

The system is pre-production with a single smoke-test domain. No migration is needed — the existing domain will be deleted and re-registered when the smoke test is re-run.

## Key Decisions

### 1. `secondaryOwnerEmails` is a required string array with minItems: 1

The field is `secondaryOwnerEmails: string[]` on the registration request, validated by the OpenAPI schema with `minItems: 1`. Each entry must be a valid email format. The Rust model uses `Vec<String>` with a deserialization check that the vector is non-empty.

Rationale: Requiring at least one secondary owner enforces the no-single-point-of-contact governance principle. Making it an array supports teams with multiple backup contacts.

### 2. Stored as a DynamoDB string set (SS)

`secondaryOwnerEmails` is persisted as a DynamoDB String Set (`SS`) attribute on the domain record. String sets are the natural DynamoDB type for a collection of unique string values.

Alternative considered: String List (`L` of `S`). Rejected because String Set enforces uniqueness natively and is more storage-efficient for email addresses where duplicates have no meaning.

### 3. Both `ownerEmail` and `secondaryOwnerEmails` are mutable via PUT /domains/{domainName}

Previously `ownerEmail` was immutable after registration. This change makes both `ownerEmail` and `secondaryOwnerEmails` mutable via the update endpoint. The `UpdateDomainRequest` schema gains optional `ownerEmail` and `secondaryOwnerEmails` fields. When present, the update handler replaces the corresponding value(s) on the domain record.

Rationale: Owners change over time — people leave, rotate, new members join. The domain record needs to stay current without requiring re-registration. Making `ownerEmail` immutable created an operational dead end when the primary owner left a team.

### 4. Included in event payloads but not in notification routing

`secondaryOwnerEmails` is added to three event payloads:
- `RegisterDomainSucceeded` — audit trail
- `DomainRegistrationApproved` — provisioning workflow input for future use
- `DomainRegistrationRejected` — audit trail

No notification Lambda sends emails to secondary owners in this change. The `send-approval-outcome-notification`, `send-rejection-outcome-notification`, and `send-provisioning-failed-notification` Lambdas continue to send to `ownerEmail` only.

Rationale: The requirement is to capture emails for informational purposes. Notification routing to secondary owners is a future enhancement that would require its own design (opt-in/opt-out, digest vs individual, etc.).

### 5. Approval workflow ASL passes the field through

The approval ASL (`approval.asl.yaml`) includes `secondaryOwnerEmails` in the `NotifyAndWait` Lambda input and in the `DomainRegistrationApproved` / `DomainRegistrationRejected` event details. No logic changes — the field flows through as data.

### 6. Smoke test email pattern

The Makefile smoke test currently uses `OWNER_EMAIL` for the domain owner, which in practice was set to the same value as `PlatformOperatorEmail` (`sliedig+nexus@example.com`). The fix:

- `OWNER_EMAIL` defaults to `sliedig+$(APP_ACCOUNT_ID)+primary@example.com` pattern in documentation
- A new `SECONDARY_OWNER_EMAIL` make variable is required for the smoke test, defaulting to `sliedig+$(APP_ACCOUNT_ID)+secondary@example.com` pattern in documentation
- The smoke test payload includes both `ownerEmail` and `secondaryOwnerEmails`

### 7. OpenAPI schema validation

The `RegisterDomainRequest` schema adds:
```yaml
secondaryOwnerEmails:
  type: array
  minItems: 1
  items:
    type: string
    format: email
```

The `UpdateDomainRequest` schema adds:
```yaml
ownerEmail:
  type: string
  format: email
secondaryOwnerEmails:
  type: array
  minItems: 1
  items:
    type: string
    format: email
```

The `DomainDetail` and `DomainSummary` schemas add:
```yaml
secondaryOwnerEmails:
  type: array
  items:
    type: string
```
(No `minItems` on response schemas — existing records may not have the field.)
