# Console development

[Contributor guide](README.md)

The console lives in `web/`. It is a Nuxt/Vue application, statically hosted on
Amazon S3 and CloudFront. Read the [Vue conventions](vue.md),
[TypeScript conventions](typescript.md), and [design system](design-system.md)
before adding a page or interaction.

## Local workflow

- `make web-dev` starts the console at `http://localhost:3000`. It writes
  `web/public/config.json` from core stack outputs and proxies `/dev/*` API
  requests to the deployed OAuth API.
- `make seed-users` creates development personas: `alice` is a platform
  administrator; `bob` is a `team-payments` owner. See [access administration](../operations/access-administration.md).
- The `/design-system` route renders the primitives against the current build.

The development proxy talks to a deployed API. Seeding users writes to Cognito.
Do not treat either as an isolated test harness. Use the
[mocked browser tests](testing.md#browser-ui-tests-playwright) for feature verification.

## Deployment

When separately authorized, `make deploy-web` builds the site, syncs the web
bucket, writes `config.json` from stack outputs, and invalidates CloudFront.
The console URL is the `KonductorWebUrl` output.

## Validation

Use the gates in [TypeScript conventions](typescript.md) and the browser and
manual accessibility checks in [testing](testing.md). Deployment success is
not evidence that interaction or accessibility checks passed.
