# Capability: Cross-Account Trust Model (delta)

## MODIFIED Requirements

### Requirement: Trust stack installation

Platform teams SHALL deploy a trust stack (CloudFormation/SAM template) in each domain account before EventBridge Nexus can manage resources in that account. The Nexus project SHALL provide this template. The trust stack SHALL create the IAM trust role with the correct permissions and trust policy. The trust policy SHALL name the platform account root as principal and SHALL restrict assumption to exactly the Nexus execution role and the provisioning workflow role via an `ArnEquals` condition on `aws:PrincipalArn`, so that the constraint is string-matched per request rather than pinned to IAM unique principal IDs. The trust role SHALL be named `{org}-nexus-trust-role`.

#### Scenario: Trust stack deployment

- **WHEN** a platform team wants to onboard a new domain account to Nexus
- **THEN** they SHALL deploy the provided trust stack template in the domain account
- **AND THEN** the template SHALL create an IAM role named `{org}-nexus-trust-role` whose trust policy permits `sts:AssumeRole` from the platform account root, conditioned via `ArnEquals` on `aws:PrincipalArn` to exactly the Nexus execution role ARN and the provisioning workflow role ARN
- **AND THEN** the role SHALL have the permissions defined in the trust role permissions requirement

#### Scenario: Assumption restricted to the named platform roles

- **WHEN** any platform-account principal other than the Nexus execution role or the provisioning workflow role attempts `sts:AssumeRole` on the trust role
- **THEN** the assumption SHALL be denied by the `aws:PrincipalArn` condition

## ADDED Requirements

### Requirement: Trust survives platform role recreation

The trust relationship between a domain account and the platform SHALL remain valid when the platform roles are deleted and recreated with the same names (for example during a core stack teardown and redeploy). Because both platform roles have deterministic names, their ARN strings are stable, and the `aws:PrincipalArn` condition SHALL continue to match without any change to the deployed trust stack.

#### Scenario: Core stack redeploy does not sever trust

- **WHEN** the platform core stack is torn down and redeployed, recreating the Nexus execution role and provisioning workflow role with new IAM unique principal IDs
- **THEN** the provisioning workflow SHALL still successfully assume `{org}-nexus-trust-role` in every domain account
- **AND THEN** no trust stack update or redeploy SHALL be required in any domain account
