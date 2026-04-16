## 1. DomainStatus serde and Display (models.rs)

- [x] 1.1 Add `#[cfg(test)]` module to `models.rs` with tests for `DomainStatus` serialization: each variant serializes to its expected SCREAMING_SNAKE_CASE string (e.g., `PendingApproval` → `"PENDING_APPROVAL"`)
- [x] 1.2 Add tests for `DomainStatus` deserialization: each SCREAMING_SNAKE_CASE string deserializes to the correct variant
- [x] 1.3 Add tests for `DomainStatus::Display` impl: each variant produces the expected string via `.to_string()`

## 2. API response helpers (api.rs)

- [x] 2.1 Add `#[cfg(test)]` module to `api.rs` with tests for `success_response`: returns correct statusCode, Content-Type header, and serialized body
- [x] 2.2 Add tests for `from_control_plane_error`: `DomainNotFound` → 404 ResourceNotFoundException, `DomainAlreadyExists` → 409 ConflictException, `ValidationException` → 400, `AccessDenied` → 403, `InvalidStateTransition` → 409, fallback → 500 InternalServerException
- [x] 2.3 Add tests for `parse_body`: valid JSON body deserializes correctly, missing body defaults to `{}`, malformed JSON returns CannotParse error
- [x] 2.4 Add tests for `path_param`: extracts value from pathParameters, returns None when missing
- [x] 2.5 Add tests for `query_param`: extracts value from queryStringParameters, returns None when missing

## 3. DynamoDB helpers (dynamodb.rs)

- [x] 3.1 Add `#[cfg(test)]` module to `dynamodb.rs` with tests for `s()`, `get_s()`, `get_m()`: correct AttributeValue construction and extraction
- [x] 3.2 Add tests for `string_map_to_attr` and `attr_to_string_map`: round-trip HashMap<String,String> through DynamoDB AttributeValue
- [x] 3.3 Add tests for `serialize_pagination_token` and `deserialize_pagination_token`: round-trip a sample key, empty input handling, malformed JSON returns None

## 4. Domain name validation (register.rs)

- [x] 4.1 Make `validate_domain_name` a `pub(crate)` function (currently private) so it can be tested from an inline test module
- [x] 4.2 Add `#[cfg(test)]` module to `register.rs` with tests: valid names pass (e.g., `"billing"`, `"my-domain"`, `"a1"`), too short fails (1 char), too long fails (65 chars), invalid characters fail (uppercase, special chars), leading/trailing hyphen fails

## 5. Notification email body extraction and tests

- [x] 5.1 In `send-approval-notification/src/main.rs`, extract the HTML body `format!()` into a `fn build_approval_email_body(domain_name, owner_account_id, owner_email, task_token) -> String` function
- [x] 5.2 Add `#[cfg(test)]` module with tests: output contains domain_name, owner_account_id, owner_email, task_token, and the heading "Approval Required"
- [x] 5.3 In `send-rejection-notification/src/main.rs`, extract the HTML body `format!()` into a `fn build_rejection_email_body(domain_name, owner_account_id, rejected_by, justification) -> String` function
- [x] 5.4 Add `#[cfg(test)]` module with tests: output contains domain_name, owner_account_id, rejected_by, justification, and the heading "Registration Rejected"

## 6. Verification

- [x] 6.1 Run `make check` and `make test` — all tests pass, no warnings
- [x] 6.2 Update decision log (`docs/development/decisions.md`) with decision: pure-function unit tests first, trait-based mocking deferred
