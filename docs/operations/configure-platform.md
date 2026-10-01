# Configure the platform

[Documentation home](../README.md)

Decide your platform settings before the first deployment. Four of them are
fixed at setup: changing one later means a new installation. This page walks
through the choices. The [configuration contract](../reference/configuration.md)
defines the exact syntax, defaults, and bounds of every setting.

## Where the settings live

All customer values live in one file, `konductor-platform.toml`, at the
repository root. The file is gitignored. Create it from the committed example:

```bash
cp konductor-platform.toml.example konductor-platform.toml
```

`make validate-config` checks the file, and `make deploy-core` validates it
again before it deploys with the same values. There is no runtime settings
store; the deployed stack parameters are the source of truth.

## Settings fixed at setup

Choose these carefully. Deployment tooling refuses to change them on an
existing installation, because they are part of resource names, trust-role
names, API stage names, and sharing scope.

| Decision | Setting | What it affects |
|---|---|---|
| Your organization's short name | `organization` | Prefixes every domain resource and the trust role name, `<org>-konductor-trust-role` |
| Which AWS Organization may register domains | `aws_organization_arn` | Scope of the AWS RAM share that publishes domain namespaces |
| Which environments every domain gets | `environments` | Every approved domain is provisioned in each listed environment; each environment is also an API stage |
| How domain resources are named | `namespace_pattern` | The names of every provisioned bus, registry, queue, and dashboard |

Guidance for each choice:

- **Organization.** Pick a short, stable identifier. Its length is capped so
  the trust role name fits within IAM's limit.
- **Environments.** List every environment you expect to need, such as
  `dev`, `staging`, and `prod`. Adding or removing an environment on a live
  platform is not supported yet. Each environment adds resources per domain,
  so review [service quotas](service-quotas.md) against the count.
- **Namespace pattern.** The default `{org}-{env}-{domain}` suits most
  installations. See [resources and naming](../reference/resources.md) for
  the names it produces.

## Settings you can change later

These change by editing the file and redeploying the core stack:

- **Notifications.** Set both `notification_sender_email` and
  `platform_operator_email` to enable approval and outcome emails, or leave
  both empty to disable them. Both addresses must be verified Amazon SES
  identities first; see
  [Phase 1 of the installation](installation.md#phase-1--deploy-the-notifications-stack-optional).
  Without notifications, administrators approve domains through the API or
  the console.
- **Access-token validity.** `access_token_validity_minutes` sets how long
  Cognito access tokens last.
- **Console redirect URLs.** `oauth_callback_urls` and `oauth_logout_urls`
  must list the addresses where the console is served. The defaults cover
  local development on `http://localhost:3000`.

## Validate and deploy

```bash
make validate-config
make deploy-core
```

Validation checks the contract rules and the SES verification status of the
notification addresses. Once a stack exists, it also refuses changes to the
fixed-at-setup settings. The [installation guide](installation.md#phase-2--deploy-the-konductor-core-stack)
covers the full deployment phase.

## Existing installations

If your installation predates the configuration file, generate it from the
deployed stack instead of retyping values. Follow
[adopt the configuration file](installation.md#existing-installations-adopt-the-configuration-file).

## Next step

[Install Konductor](installation.md).
