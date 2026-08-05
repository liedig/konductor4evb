## MODIFIED Requirements

### Requirement: Decide Registration

Platform team members SHALL be able to approve or reject a domain registration request only through the OAuth control-plane surface. The synchronous endpoint SHALL require an authenticated trusted principal with the `platform-admin` global role. It SHALL accept the Step Functions task token, decision, and justification, but SHALL derive `decidedBy` from the trusted OAuth principal rather than a caller-asserted request field. Possession of a task token SHALL NOT authorize the caller. The Lambda SHALL call `SendTaskSuccess` or `SendTaskFailure` to resume the approval workflow and SHALL NOT read from DynamoDB, emit events, or update domain status.

#### Scenario: Approval decision submitted successfully

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/registrations/decide` with a valid task token, decision of `ACCEPT`, and optional justification
- **THEN** the decide Lambda SHALL call `sfn:SendTaskSuccess` with the task token and an output payload containing `approved: true`, trusted principal-derived `decidedBy`, and `justification`
- **AND THEN** the Lambda SHALL return `200 OK` with `{"message":"Decision recorded"}`

#### Scenario: Rejection decision submitted successfully

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/registrations/decide` with a valid task token, decision of `REJECT`, and required justification
- **THEN** the decide Lambda SHALL call `sfn:SendTaskFailure` with the task token, error code `RegistrationRejected`, and cause containing trusted principal-derived `decidedBy` and `justification`
- **AND THEN** the Lambda SHALL return `200 OK` with `{"message":"Decision recorded"}`

#### Scenario: Bearer task token is not caller authorization

- **WHEN** a caller presents a valid task token without a valid OAuth principal carrying `platform-admin`
- **THEN** API Gateway SHALL reject the request before invoking the decide Lambda
- **AND THEN** no Step Functions callback SHALL be made

#### Scenario: Caller asserts a decider identity

- **WHEN** a decision request contains a caller-supplied `decidedBy` value
- **THEN** that value SHALL NOT be used as the authoritative decider identity
- **AND THEN** the workflow callback and audit record SHALL identify the validated OAuth principal

#### Scenario: Decision with missing or invalid fields

- **WHEN** an authorized caller submits a decision with a missing task token or decision, or an invalid decision value
- **THEN** the request SHALL be rejected with `400 Bad Request` and a descriptive validation error
- **AND THEN** no Step Functions callback SHALL be made

#### Scenario: Decision with invalid or expired task token

- **WHEN** an authorized caller submits a decision with a task token that is expired, already used, or invalid
- **THEN** the `sfn:SendTaskSuccess` or `sfn:SendTaskFailure` call SHALL fail
- **AND THEN** the Lambda SHALL return a safe error response indicating that the callback failed

#### Scenario: Justification required on rejection

- **WHEN** an authorized caller submits a decision of `REJECT` without a `justification` field
- **THEN** the Lambda SHALL return `400 Bad Request` indicating that justification is required for rejections
- **AND THEN** no Step Functions callback SHALL be made
