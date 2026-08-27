## 1. Notifications Stack Template

- [x] 1.1 Create `notifications-stack.yaml` with Parameters (Organization, Environment, FromEmail, ToEmail), `AWS::SES::EmailIdentity` for From and To, and Outputs (FromEmail, ToEmail)
- [x] 1.2 Add same-region constraint to the template Description and parameter help text
- [x] 1.3 Add `samconfig.toml` profile for notifications stack deploy (`acme-dev-konductor-notifications`)

## 2. Core Template Conditional Notifications

- [x] 2.1 Change `ApprovalFromEmail` and `ApprovalToEmail` parameters to optional with empty-string defaults in `template.yaml`
- [x] 2.2 Add `NotificationsEnabled` CloudFormation condition (`!Not [!Equals [!Ref ApprovalFromEmail, ""]]`)
- [x] 2.3 Guard `SendApprovalNotificationFunction`, its log group, and IAM permissions behind `NotificationsEnabled` condition
- [x] 2.4 Guard `SendRejectionNotificationFunction`, its log group, and IAM permissions behind `NotificationsEnabled` condition
- [x] 2.5 Guard `DomainRegistrationRejectedRule` and its Lambda permission behind `NotificationsEnabled` condition
- [x] 2.6 Guard the approval workflow role's `lambda:InvokeFunction` permission for `SendApprovalNotificationFunction` behind `NotificationsEnabled` condition
- [x] 2.7 Remove SES-specific environment variable references from notification Lambda configs when `NotificationsEnabled` is false (use `Condition` on the function resource)

## 3. Samconfig Naming Alignment

- [x] 3.1 Update `samconfig.toml` default and dev profile `stack_name` to `acme-dev-konductor`
- [x] 3.2 Verify the dev profile `parameter_overrides` still matches the new optional parameter defaults

## 4. Trust Stack Doc Fix

- [x] 4.1 Fix `docs/trust-stack-deployment.md` parameter names to match `trust-stack.yaml` (`KonductorRoleArn` not `ControlPlaneRoleArn`)
- [x] 4.2 Fix stack name examples in trust-stack doc to `{org}-{env}-konductor-trust` pattern
- [x] 4.3 Fix trust role name references to `${Organization}-${Environment}-konductor-trust-role`

## 5. Documentation Updates

- [x] 5.1 Update `docs/development/decisions.md` with the notifications-sibling-stack decision and same-region constraint
- [x] 5.2 Update `docs/development/architecture.md` to reflect the three-stack deployment model (notifications, core, trust)

## 6. Verification

- [x] 6.1 Run `make check` and `make test` to confirm no Rust code regressions
- [x] 6.2 Run `sam validate --template-file notifications-stack.yaml` to confirm valid template
- [x] 6.3 Run `sam validate` on core template with `ApprovalFromEmail=""` `ApprovalToEmail=""` to confirm valid template without notifications
- [x] 6.4 Run `sam validate` on core template with non-empty email params to confirm notification resources are included
