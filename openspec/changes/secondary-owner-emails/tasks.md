## Task Group 1: OpenAPI Spec

- [x] 1.1 Add `secondaryOwnerEmails` (array, minItems: 1, items: string format email) to `RegisterDomainRequest` schema in `api/openapi.yaml`
- [x] 1.2 Add `secondaryOwnerEmails` (array, minItems: 1, items: string format email) and `ownerEmail` (string, format email) to `UpdateDomainRequest` schema
- [x] 1.3 Add `secondaryOwnerEmails` (array, items: string) to `DomainDetail` response schema (no minItems — existing records may lack the field)
- [x] 1.4 Add `secondaryOwnerEmails` (array, items: string) to `DomainSummary` response schema

## Task Group 2: Rust Models

- [x] 2.1 Add `secondary_owner_emails: Vec<String>` to `RegisterDomainRequest` in `lib/nexus-common/src/models.rs`
- [x] 2.2 Add `secondary_owner_emails: Option<Vec<String>>` to `Domain` struct
- [x] 2.3 Add `secondary_owner_emails: Option<Vec<String>>` and `owner_email: Option<String>` to `UpdateDomainMessage` struct in `domain-management/functions/domain-registrations/src/update.rs` (or equivalent update request type)
- [x] 2.4 Add a test verifying `RegisterDomainRequest` rejects an empty `secondaryOwnerEmails` array
- [x] 2.5 Add a test verifying `RegisterDomainRequest` rejects a missing `secondaryOwnerEmails` field

## Task Group 3: Register Handler

- [x] 3.1 `register.rs`: Persist `secondaryOwnerEmails` as a DynamoDB String Set (`SS`) in the `PutItem` call
- [x] 3.2 `register.rs`: Include `secondaryOwnerEmails` in the `RegisterDomainSucceeded` event payload
- [x] 3.3 `register.rs`: Include `secondaryOwnerEmails` in the Step Functions `StartExecution` input for the approval workflow
- [x] 3.4 `register.rs`: Update all test fixtures to include `secondary_owner_emails`

## Task Group 4: Describe Handler

- [x] 4.1 `describe-domain/main.rs`: Extract `secondaryOwnerEmails` from the DynamoDB item and include it in the JSON response

## Task Group 5: List Handler

- [x] 5.1 `list-domains/main.rs`: Extract `secondaryOwnerEmails` from each DynamoDB item and include it in the response items

## Task Group 6: Update Handler

- [x] 6.1 `update.rs`: Accept `ownerEmail` and `secondaryOwnerEmails` in the update message; update `ownerEmail` as a String and replace the `secondaryOwnerEmails` String Set on the domain record when either is present
- [x] 6.2 `update.rs`: Include `ownerEmail` and/or `secondaryOwnerEmails` in the `changedFields` of `UpdateDomainSucceeded` event when the respective fields were updated
- [x] 6.3 `update.rs`: Update test fixtures to include `secondary_owner_emails`

## Task Group 7: Approval Workflow

- [x] 7.1 `approval.asl.yaml`: Include `secondaryOwnerEmails` in the `NotifyAndWait` Lambda input
- [x] 7.2 `approval.asl.yaml`: Include `secondaryOwnerEmails` in the `DomainRegistrationApproved` event detail
- [x] 7.3 `approval.asl.yaml`: Include `secondaryOwnerEmails` in the `DomainRegistrationRejected` event detail

## Task Group 8: Documentation

- [x] 8.1 Update `docs/domain-management.md`: Add `secondaryOwnerEmails` to the registration sample payload, field reference table, describe response example, and update mutable fields table
- [x] 8.2 Update `docs/development/architecture.md`: Add `secondaryOwnerEmails` row to the data model table schema

## Task Group 9: Smoke Test

- [x] 9.1 Update `Makefile` smoke target: Add `SECONDARY_OWNER_EMAIL` variable with `$(error ...)` guard, include `secondaryOwnerEmails` in the registration request payload
- [x] 9.2 Update `Makefile` `help` target: List `SECONDARY_OWNER_EMAIL` as required for smoke
- [x] 9.3 Update `docs/installation.md`: Change smoke test examples to use `sliedig+{account}+primary@example.com` for `OWNER_EMAIL` and `sliedig+{account}+secondary@example.com` for `SECONDARY_OWNER_EMAIL`

## Task Group 10: Build and Test

- [x] 10.1 `cargo test --workspace` — all tests pass (108 passed)
- [x] 10.2 `cargo fmt --check && cargo clippy --workspace -- -D warnings` — clean
- [x] 10.3 `sam build --config-file samconfig-core.toml --beta-features -c` — succeeds

## Task Group 11: Deploy and Verify

- [x] 11.1 Delete the existing smoke-test domain record from DynamoDB and clean up stale resources (event buses, schema registries, DLQs) from the previous deployment
- [x] 11.2 Deploy the core stack (Lambda code changes only, no table schema change)
- [x] 11.3 Run `make smoke APP_ACCOUNT_ID=<id> OWNER_EMAIL=sliedig+demo+primary@example.com SECONDARY_OWNER_EMAIL=sliedig+demo+secondary@example.com` and verify the domain reaches ACTIVE
- [x] 11.4 Verify `GET /domains/{domainName}` response includes `secondaryOwnerEmails` with the correct values
