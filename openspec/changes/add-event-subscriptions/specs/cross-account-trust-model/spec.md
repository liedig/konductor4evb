# Capability: Cross-Account Trust Model (delta)

## ADDED Requirements

### Requirement: Platform delivery authorization on app-side buses

Each app-side domain event bus SHALL carry a resource policy permitting the **platform account** to call `events:PutEvents`, attached once at provisioning time via the trust role's existing `events:PutPermission` capability. This single grant SHALL authorize delivery for all current and future subscriptions targeting that bus — no per-subscription IAM roles or policies SHALL be created. Subscription rules in the platform account SHALL target app-side buses directly under this authorization.

#### Scenario: Policy attached during environment provisioning

- **WHEN** the per-environment provisioning workflow creates the app-side domain event bus in the domain account
- **THEN** it SHALL attach a resource policy to that bus permitting `events:PutEvents` from the platform account
- **AND THEN** the policy SHALL be attached using the trust role's existing `events:PutPermission` grant, requiring no trust-stack change

#### Scenario: Subscriptions require no additional authorization

- **WHEN** a subscription is created or removed for a consumer domain
- **THEN** no IAM roles, role policies, or resource policy changes SHALL be made in either account
- **AND THEN** unsubscribing SHALL leave the provision-time delivery policy in place for other and future subscriptions

## REMOVED Requirements

### Requirement: Bus-to-bus assumed roles for subscription routing

**Reason**: Replaced by the provision-time platform delivery resource policy on app-side buses. Per-subscription bus-to-bus roles added IAM proliferation and shared-role cleanup accounting on unsubscribe without adding protection — the platform account is the only cross-account sender any app-side bus will ever receive from, so an account-level grant carries the same effective trust boundary. This requirement was specified but never implemented; no runtime migration is needed.

**Migration**: New domains receive the delivery policy during `provision-env`. No backfill exists: the product is unreleased and no environment carries pre-change domains. The trust stack's `BusToBusRoleManagement` policy (`{org}-*-b2b-*` roles) becomes unused by subscription routing and MAY be retired from the trust stack in a future change.
