## ADDED Requirements

### Requirement: Ownership-aware domain resource references

The domain detail page SHALL retain all provisioned resource identifiers as read-only reference information and SHALL distinguish platform-account resources from resources in the domain account. It SHALL identify both groups as Konductor-managed infrastructure. Platform event buses, schema registries, subscription DLQs, and dashboards SHALL NOT have AWS console links on this page. Recognized application event buses and integration DLQs SHALL have console links only when the identifier's account matches the domain owner account. Console navigation SHALL NOT be described as granting access or enforcing a read-only AWS session.

#### Scenario: Platform identifiers remain useful without inaccessible links

- **WHEN** a domain owner expands an environment with platform resources
- **THEN** all platform resource identifiers SHALL remain visible as text without console anchors
- **AND** the resources SHALL be distinguished from resources in the domain account

#### Scenario: Domain-account resources offer console navigation

- **WHEN** an application event bus ARN or integration DLQ queue URL identifies the domain's owner account
- **THEN** the page SHALL offer a console link to that recognized resource
- **AND** the identifier SHALL remain visible

#### Scenario: Unknown or inconsistent identifiers fail safely

- **WHEN** a resource identifier is unknown, malformed, missing account information, or inconsistent with the domain owner account for a link-eligible field
- **THEN** the page SHALL NOT generate a console link for it
- **AND** any supplied identifier SHALL remain visible as text

#### Scenario: Co-located platform resources remain platform references

- **WHEN** a platform resource's account happens to equal the domain owner account
- **THEN** the platform resource SHALL still be displayed without a console link
- **AND** account equality SHALL NOT reclassify its management purpose
