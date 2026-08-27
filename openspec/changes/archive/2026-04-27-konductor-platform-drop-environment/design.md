# Design: konductor-platform-drop-environment

## Context

Konductor today carries an `Environment` parameter at the platform level, defaulting to `dev`. This comes from an architecture decision captured early in the project that treated Konductor itself as something one might deploy per-environment, by analogy with typical application stacks. In practice Konductor is an integration platform — a singleton per organisation, deployed once into a dedicated platform account. The "environments" it cares about are the environments of the **domains** it provisions, not of itself.

The conflation leaked into every resource name (`acme-dev-konductor-bus`, `acme-dev-domains`, `acme-dev-konductor-trust-role`) and into every file in the stack. Removing it now, while Konductor is pre-production, avoids baking the confusion into operational resources that would later need migration.

This change follows `multi-env-domain-provisioning`. That prior change has already (a) removed `${Environment}` from the provisioning state machine by sourcing env per Map iteration from the domain record, and (b) renamed the trust role in application accounts to `{org}-konductor-trust-role`. This change removes the now-orphaned `Environment` parameter from the Konductor platform templates and Makefile/samconfig and renames platform-account resources.

## Decisions

### D1: Platform resource naming becomes `{org}-konductor-*`

All resources defined by the Konductor platform templates — the DynamoDB domains table, the platform event bus, the API Gateway, Lambda functions, command queues and DLQs, log groups, dashboards, IAM roles — are renamed from `${Organization}-${Environment}-<thing>` to `${Organization}-konductor-<thing>`.

The `-konductor-` segment is deliberate. Using just `{org}-<thing>` would cause naming collisions with domain resources (which follow `{org}-{env}-{domain}-*`) if a domain were ever named `konductor`; and it would make it harder to scan an AWS console for "what is a Konductor platform resource vs. a domain resource". The explicit `-konductor-` is worth the extra characters.

Rejected: `{org}-ctrl-*`, `{org}-cp-*` (cryptic); `konductor-*` without the org prefix (breaks multi-org scanning in shared accounts).

### D2: API Gateway stage becomes `v1`

`StageName: !Ref Environment` becomes `StageName: v1`. This gives us a static, meaningful stage name that doubles as a foundation for API versioning if the contract ever needs to evolve in an incompatible way.

Rejected: `api` (redundant with the `/konductor-api` service name); `prod` (implies an env concept that doesn't apply); removing the stage concept entirely (requires a larger API Gateway rework for no win).

### D3: `NamespacePattern` parameter stays, re-scoped to domain resources only

The template currently has:

```yaml
NamespacePattern:
  Type: String
  Default: "{org}-{env}-{domain}"
  Description: Configurable namespace convention for resource naming
```

It's threaded into Lambda env vars so the namespace resolver can use it for domain resource names. Domain resources **do** need env scoping, so the pattern is still meaningful — but only for them. We keep the parameter and update its description to say "for domain resource naming" explicitly. Platform resources use the fixed `${Organization}-konductor-*` pattern.

This is the minimal change. Deleting `NamespacePattern` would be a YAGNI cleanup but would force a second change to add it back when a customer inevitably asks for a non-default convention. Keeping the extensibility hook with a clearer description is cheap.

### D4: Makefile defaults and guards

```make
AWS_REGION ?= $(shell aws configure get region 2>/dev/null || echo us-east-1)
```

For required vars with no sane default, use make's `$(error …)` pattern so they fail at parse time with a clear message:

```make
deploy-notifications:
	$(if $(FROM_EMAIL),,$(error FROM_EMAIL is required. Usage: make deploy-notifications FROM_EMAIL=... TO_EMAIL=...))
	$(if $(TO_EMAIL),,$(error TO_EMAIL is required. Usage: make deploy-notifications FROM_EMAIL=... TO_EMAIL=...))
	aws cloudformation deploy ...
```

The guards go only on targets that need the variable. `make build` should not fail for missing `OWNER_EMAIL`.

Rejected: `?=` defaults of `CHANGEME@example.com` style placeholders — silently deploying with a bad address is strictly worse than a clear early error.

### D5: `AwsOrganizationArn` moves into samconfig

`AwsOrganizationArn` is a required template parameter with no default. Today samconfig doesn't set it, so `make deploy-core` requires a manual `--parameter-overrides AwsOrganizationArn=…`. The pragmatic fix is to require the operator to set it once in `samconfig-core.toml`:

```toml
[default.deploy.parameters]
parameter_overrides = "Organization=acme AwsOrganizationArn=arn:aws:organizations::123456789012:organization/o-abc123"
```

The installation guide already tells the operator to capture their Org ARN during prerequisites; this just gives them a concrete place to put it.

Rejected: auto-discovering the Org ARN via `aws organizations describe-organization` in the Makefile — adds an IAM permission requirement and couples `make deploy-core` to extra API calls.

## Risks

- **CloudFormation resource rename = delete+create.** Anyone who has already deployed Konductor will have their domains table deleted along with all its records. The project is pre-production, so this is accepted. If that assumption changes, the design needs a data-migration section.
- **`${Organization}-konductor-*` pattern collides with a hypothetical domain named `konductor`.** Domain-management validation SHALL reject `konductor` as a reserved domain name. Adding this validation is called out in tasks.

## Open Questions

None. All decisions above are settled.
