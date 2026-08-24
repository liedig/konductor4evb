## ADDED Requirements

### Requirement: Governed domain tag model

Every domain SHALL carry a standard tag set, applied by the platform: `konductor:managed-by` (`konductor`), `konductor:domain` (the domain name), `konductor:environment` (per environment on per-environment resources), and `konductor:namespace` (the `{org}-{domain}` namespace identity). A registration MAY additionally supply a single governed custom tag, `costCenter`, for cost allocation; the API SHALL reject any other custom tag keys. The domain record SHALL store the effective tag set (standard set plus `cost-center` when supplied), and describe and list responses SHALL render it. `cost-center` SHALL be settable at registration and SHALL NOT be editable afterwards in this capability version.

#### Scenario: Registration with a cost center

- **WHEN** a caller submits a registration with `costCenter: "cc-1234"`
- **THEN** the record SHALL persist `costCenter`
- **AND THEN** describe SHALL return the standard tag set plus `cost-center=cc-1234` as the effective tags

#### Scenario: Arbitrary tag keys rejected

- **WHEN** a caller submits a registration with a `tags` map or any custom key other than `costCenter`
- **THEN** the API SHALL return `400 ValidationException`

#### Scenario: No cost center

- **WHEN** a registration omits `costCenter`
- **THEN** the record SHALL carry the standard tag set only and registration SHALL proceed normally

#### Scenario: Cost center is immutable

- **WHEN** an owner submits a domain update carrying `costCenter`
- **THEN** the API SHALL return `400 ValidationException`
