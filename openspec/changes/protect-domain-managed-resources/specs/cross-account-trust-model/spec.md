## ADDED Requirements

### Requirement: Organization-enforced managed infrastructure protection

Konductor SHALL provide a separately deployable organization-admin guardrail package using AWS Organizations SCPs to protect its application-account event buses and integration DLQs. When attached to participating member accounts, the guardrails SHALL deny unauthorized resource deletion, configuration and policy changes, queue purge, and protected ownership-identifier mutation, including operations by member-account administrators and root within SCP coverage. The guardrails SHALL preserve permitted integration rules and queue consumption and SHALL NOT grant IAM permissions themselves.

#### Scenario: Broad administrator permissions do not bypass protection

- **WHEN** a member-account principal with broad administrator permissions attempts a protected operation without matching an authorized platform exception
- **THEN** the attached SCP guardrail SHALL deny that operation despite the identity-based allow

#### Scenario: Resource identity cannot be removed to evade enforcement

- **WHEN** a local principal attempts to remove or forge ownership tags or recreate a protected resource in a reserved namespace
- **THEN** the guardrails SHALL prevent that action from bypassing the managed-resource restrictions

#### Scenario: Member accounts cannot shed protection through organization departure

- **WHEN** a local member-account principal attempts to leave the organization to escape the attached guardrails
- **THEN** the centrally administered guardrails SHALL deny that departure within the supported Organizations enforcement model

#### Scenario: Konductor lifecycle operations remain authorized

- **WHEN** Konductor uses the designated trusted platform path to provision, compensate, deprovision, or retag managed infrastructure
- **THEN** the guardrails SHALL permit the required operations subject to the existing role permissions
- **AND** unrelated local principals SHALL NOT inherit this exception

### Requirement: Protected platform exception identities

Platform exceptions SHALL identify exact configured roles rather than broad name patterns. Domain-account principals SHALL NOT be able to acquire an exception by modifying or recreating an exempt role, changing its trust or permissions policies, attaching policies, changing relevant managed-policy versions, passing the role to another service, or modifying associated ownership or boundary controls. The ordinary Konductor resource-management path SHALL NOT authorize self-modification of its trust policy.

#### Scenario: Local administrator cannot rewrite the trust role

- **WHEN** a local administrator attempts to change the exempt role's trust policy or effective permissions
- **THEN** the operation SHALL be denied outside the designated centrally controlled maintenance path

#### Scenario: Role replacement and passing do not create exceptions

- **WHEN** a local principal attempts to delete and recreate an exempt role, pass it to a service, or assume it without meeting the configured platform trust conditions
- **THEN** the attempt SHALL be denied

#### Scenario: Authorized role sessions retain the intended exception

- **WHEN** an authorized platform principal assumes the exact configured Konductor trust role
- **THEN** its session SHALL receive only the intended resource-management exception
- **AND** similarly named roles SHALL NOT receive that exception

### Requirement: Central recovery and explicit enforcement coverage

Guardrail installation SHALL require explicit organization-admin targeting and a verified centrally controlled recovery path that domain-account administrators cannot modify or assume. Normal domain registration and provisioning SHALL NOT attach or modify organization policies. Installation guidance SHALL distinguish SCP restrictions from application IAM grants, SHALL document management-account and service-linked-role exclusions, and SHALL NOT claim deployed enforcement based solely on local tests, console rendering, or trust-role presence.

#### Scenario: Recovery remains possible without local bypass

- **WHEN** a guardrail blocks a legitimate platform maintenance operation
- **THEN** centrally authorized operators SHALL have a documented procedure to repair policy or role configuration
- **AND** the procedure SHALL NOT grant domain administrators a standing bypass

#### Scenario: Enforcement evidence is missing

- **WHEN** only local policy tests have run or target-account attachment has not been verified
- **THEN** the installation SHALL be described as not yet verified for deployed enforcement
- **AND** separately authorized sandbox checks SHALL be required before claiming the protection operational

#### Scenario: An account or principal is outside SCP coverage

- **WHEN** a deployment uses the management account or an AWS-documented SCP-exempt principal path
- **THEN** the documented protection claim SHALL explicitly exclude that path
- **AND** the presence of this guardrail package SHALL NOT be presented as removing that AWS limitation
