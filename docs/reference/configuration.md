# Platform configuration contract

[Documentation home](../README.md)

Every customer-supplied platform setting, its syntax, default, and lifecycle
class. Deployment parameters are the single source of truth; there is no
runtime settings store. Fixed-at-setup settings cannot change on an existing
installation — changing them means a new deployment. Updateable settings change
by redeploying with new values.

## Settings

| Setting | Parameter | Syntax | Default | Lifecycle |
|---|---|---|---|---|
| Organisation identifier | `Organization` | 2–43 chars: `^[a-z0-9]([a-z0-9-]{0,41}[a-z0-9])$` | none (required) | Fixed at setup |
| Domain environments | `Environments` | 1–10 entries, comma-delimited without spaces, each `^[a-z0-9][a-z0-9-]{0,31}$`, unique | `dev,staging,prod` | Fixed at setup |
| Resource namespace pattern | `NamespacePattern` | `{org}`, `{env}`, `{domain}` each required; literal chars lowercase/digit/hyphen only; ≤ 128 chars | `{org}-{env}-{domain}` | Fixed at setup |
| AWS Organization ARN | `AwsOrganizationArn` | `^arn:aws[a-z-]*:organizations::\d{12}:organization/o-[a-z0-9]{10,32}$` | none (required for RAM sharing) | Fixed at setup |
| Notification sender | `NotificationSenderEmail` | Email address; set both or neither with the operator recipient; SES-verified before enabling | empty (notifications disabled) | Updateable |
| Platform operator recipient | `PlatformOperatorEmail` | Email address; paired with the sender; SES-verified before enabling | empty (notifications disabled) | Updateable |
| Access-token validity | `AccessTokenValidityMinutes` | Integer 5–1440 | `60` | Updateable |
| OAuth callback URLs | `OAuthCallbackUrls` | Comma-delimited URLs; HTTPS except `http://localhost[:port]` | `http://localhost:3000/callback` | Updateable |
| OAuth logout URLs | `OAuthLogoutUrls` | Comma-delimited URLs; HTTPS except `http://localhost[:port]` | `http://localhost:3000/logout` | Updateable |
| System-event emission | `SystemEventEmissionEnabled` | Boolean | `false` | Updateable |
| Publish throttle rate | `PublishThrottleRateLimit` | Integer ≥ 1 (requests per second, per stage) | `2000` | Updateable |
| Publish throttle burst | `PublishThrottleBurstLimit` | Integer ≥ 0 (per stage) | `1000` | Updateable |
| Publish admission concurrency cap | `PublishAdmissionConcurrencyCap` | Integer ≥ 0; `0` means uncapped | `0` | Updateable |
| Publish delivery max concurrency | `PublishDeliveryMaxConcurrency` | Integer 2–1000 | `50` | Updateable |
| Domain liveness cache window | `DomainLivenessCacheSeconds` | Integer 0–900; `0` means a read on every request | `300` | Updateable |

## Why these bounds

- **Organisation identifier (≤ 43 chars).** The identifier prefixes every
  resource name. The tightest limit is the 64-character IAM role-name maximum
  for the trust role `<org>-konductor-trust-role` deployed into each domain
  account, leaving 43 characters for the identifier.
- **Environment entries.** The per-entry rule matches what domain registration
  enforces at runtime (`^[a-z0-9][a-z0-9-]{0,31}$`). The entry count is bounded
  by the API Gateway stages-per-API quota (10 by default; adjustable), because
  each environment is served as an API stage.
- **Namespace pattern charset.** Literal characters are restricted to
  lowercase, digits, and hyphens so every substituted name stays valid for the
  strictest services in use (SQS queue names allow only `[A-Za-z0-9_-]`).
  Placeholder *presence* is checked by preflight validation; the template
  enforces the charset.
- **System-event emission.** The rollout gate for [system events](system-events.md):
  deploy with `false`, stand up the system-events infrastructure, then flip to
  `true`. While `false`, re-pin lifecycles behave exactly as before and write
  no upgrade snapshots.
- **Publish throttle.** `POST /publish` gets a method-level throttle on every
  environment stage of both APIs; every other route keeps the stage-wide
  50 rps / 100 burst. The throttle is a ceiling against the account's shared,
  per-Region API Gateway throttle, not a reservation: API Gateway applies the
  stage-wide limit to each method independently, so the control-plane budget
  is the method count times the limit. `make validate-config` warns when one
  stage at its publish limit plus every stage's control-plane ceiling would
  exceed the account quota (see
  [service quotas](../operations/service-quotas.md#the-publish-throttle-budget)).
- **Admission concurrency cap.** `0` (the default) means admission draws from
  the account's shared Lambda pool — a reserved concurrency of 0 would throttle
  every invocation, so 0 can never be a useful cap. A positive value isolates
  admission from, and bounds its impact on, the shared pool; size it against
  the Schemas API quota, because each cold instance fetches its schemas from
  the registry.
- **Delivery max concurrency.** Enforced through the SQS event source
  mapping's `ScalingConfig`, so saturated delivery leaves messages queued
  instead of raising their receive counts toward the DLQ. 2–1000 is the AWS
  range for the mapping. Size it against the Region's `PutEvents` quota.
- **Domain liveness cache window.** Admission and delivery may reuse a
  confirmed-`ACTIVE` result for this many seconds, per warm instance. A
  disable takes effect within the window; `INACTIVE` and missing domains are
  never cached, and `0` disables reuse (a read on every request). Capped at
  900 so a disable always takes effect promptly.
- **Fixed versus updateable.** The fixed four are baked into resource names,
  trust-role names in every domain account, RAM share scope, API stage names,
  and Lambda environment. Changing them on a live installation would rename,
  replace, or orphan resources, so deployment tooling refuses the change
  instead. Adding or removing environments on a live platform is a
  provisioning-shaped migration deferred to a future change.

## Stage rendering

The core stack serves one API stage per environment, generated from the
`environments` list at deploy time. `make build` and `make deploy-core` run
`validate-config.py --render-stages`. When the authoring environment (the
first entry) is not the default `dev`, the script rewrites the two marked
`StageName` lines in `konductor-core-stack.yaml`. It always regenerates the
marked rendered-stages block, with one additional stage resource per
remaining environment. The template is committed with the default list
rendered, so default installations see no diff; a custom list leaves a
working-tree change to commit alongside the configuration change.
`validate-config.py --check-stages` (run in CI) fails when the committed
template drifts from the rendered output. The block renders inline rather
than through `Fn::Transform`/`AWS::Include` — a Resources-level include
failed changeset creation in the full template.

## Related

- [Installation](../operations/installation.md) — where the values are supplied.
- [Resources and naming](resources.md) — what the namespace pattern produces.
- [Service quotas](../operations/service-quotas.md) — quota headroom the
  environment count feeds into.
