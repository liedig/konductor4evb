## 1. DynamoDB table schema

- [x] 1.1 Change `DomainsTable` partition key in `konductor-core-stack.yaml` from `domainId` (S) to `domainName` (S), including `KeySchema` and `AttributeDefinitions`
- [x] 1.2 Remove the `by-name` GSI from the `DomainsTable` `GlobalSecondaryIndexes` list
- [x] 1.3 Remove `domainId` from the `AttributeDefinitions` list (no longer used as a key or GSI key)

## 2. OpenAPI spec

- [x] 2.1 Change path parameter from `{domainId}` to `{domainName}` on the `GET /domains/{domainName}` and `PUT /domains/{domainName}` operations in `api/openapi.yaml`
- [x] 2.2 Update the `PUT /domains/{domainName}` VTL mapping template to inject `domainName` instead of `domainId` into the SQS message body
- [x] 2.3 Remove `domainId` from the `DomainSummary` and `DomainDetail` response schemas

## 3. Lambda functions

- [x] 3.1 `register.rs`: Remove UUID generation, use `domainName` as the `PutItem` partition key with a conditional expression `attribute_not_exists(domainName)`, catch `ConditionalCheckFailedException` and emit `RegisterDomainFailed` with "domain already exists" reason, remove the pre-write `by-name` GSI query
- [x] 3.2 `register.rs`: Remove `domainId` from the `RegisterDomainSucceeded` event payload and the Step Functions `StartExecution` input
- [x] 3.3 `describe-domain/main.rs`: Change `GetItem` key from `domainId` to `domainName`, extract `domainName` from path parameters instead of `domainId`
- [x] 3.4 `describe-domain/main.rs`: Remove `domainId` from the response JSON body
- [x] 3.5 `update.rs`: Change `GetItem` and `UpdateItem` key from `domainId` to `domainName`, update `UpdateDomainMessage` struct to use `domainName` field instead of `domain_id`
- [x] 3.6 `update.rs`: Remove `domainId` from the `UpdateDomainSucceeded` and `UpdateDomainFailed` event payloads
- [x] 3.7 `list-domains/main.rs`: Remove `domainId` from the response item JSON body
- [x] 3.8 `domain-registrations/main.rs`: Update the SQS message dispatch to pass `domainName` (not `domainId`) to the register and update handlers

## 4. Shared library (konductor-common)

- [x] 4.1 `models.rs`: Remove `domain_id` field from the `Domain` struct
- [x] 4.2 `models.rs`: Remove `domainId` references from any serialization/deserialization tests
- [x] 4.3 `dynamodb.rs`: Update any helper functions that reference `domainId` as a key name

## 5. Step Functions state machines

- [x] 5.1 `approval.asl.yaml`: Replace `domainId` with `domainName` in workflow inputs, notification Lambda parameters, and DynamoDB update expressions
- [x] 5.2 `provision-domain.asl.yaml`: Replace `domainId` with `domainName` in workflow inputs, DynamoDB update expressions, and event payloads (`DomainProvisioningSucceeded`, `DomainProvisioningFailed`)
- [x] 5.3 `provision-env.asl.yaml`: Replace `domainId` with `domainName` in workflow input parameters
- [x] 5.4 `deprovision-env.asl.yaml`: Replace `domainId` with `domainName` in workflow input parameters
- [x] 5.5 Update `DefinitionSubstitutions` in `konductor-core-stack.yaml` for all state machines to remove any `domainId`-related substitutions

## 6. Smoke test

- [x] 6.1 Update `Makefile` smoke target to use `GET /domains/{domainName}` instead of polling DynamoDB for `domainId`, and use `domainName` in the describe and decide API calls

## 7. Documentation

- [x] 7.1 Update `docs/installation.md` manual smoke test examples to use `/domains/{domainName}` paths and remove references to `domainId`
- [x] 7.2 Update `README.md` API section to reflect `domainName` path parameters and remove `domainId` from endpoint descriptions
- [x] 7.3 Update `docs/development/decisions.md` with decision to use `domainName` as primary key, documenting the DDD/EDA rationale and the account-per-domain principle
- [x] 7.4 Update `docs/development/architecture.md` to document the account-per-domain design principle and primary/secondary owner model

## 8. Build and test

- [x] 8.1 Run `cargo test --workspace` and fix any test failures from `domainId` removal
- [x] 8.2 Run `cargo fmt --check && cargo check --workspace && cargo clippy --workspace -- -D warnings` and fix any issues
- [x] 8.3 Run `sam build --config-file samconfig-core.toml -c` and verify the build succeeds
- [x] 8.4 Deploy the core stack to the platform account and verify the DynamoDB table is recreated with `domainName` as the partition key

## 9. End-to-end verification

- [x] 9.1 Refresh demo credentials via SSO and deploy the trust stack to the demo account
- [x] 9.2 Run `make smoke APP_ACCOUNT_ID=210987654321 OWNER_EMAIL=sliedig+konductor@example.com AWS_REGION=ap-southeast-2` and verify the domain reaches ACTIVE status using the domain name as the identifier throughout
- [x] 9.3 Verify `GET /domains/{domainName}` returns the domain detail with `resourceArns` and no `domainId` field
