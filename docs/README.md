# Konductor documentation

Konductor for Amazon EventBridge manages event domains and cross-account routing.
Platform teams govern the infrastructure. Application teams publish events and
subscribe to contracts without managing the platform's delivery resources.

## How Konductor works

Konductor runs in two kinds of AWS account:

- The **platform account** hosts one Konductor installation: the APIs, the
  approval and provisioning workflows, the schema registries, and the
  platform-side event buses.
- **Domain accounts** belong to application teams. Each one carries a trust
  role that lets Konductor create the team's application-side event buses and
  queues.

Work moves through three stages:

1. A platform administrator installs and configures Konductor, deploys the
   trust role into domain accounts, and sets up users.
2. An application team registers a domain. A platform administrator approves
   it, and Konductor provisions its resources in every configured environment.
   The domain becomes `ACTIVE`.
3. Producers publish events to their domain through the API. Consumers
   subscribe to a producer's schema, and Konductor delivers matching events to
   the consumer's own event bus.

The [architecture overview](architecture/README.md) explains the boundaries
behind these stages.

## Application teams: register and use a domain

1. [Check the domain prerequisites](guides/domain-prerequisites.md) with your platform administrator.
2. [Authenticate API requests](guides/authentication.md).
3. [Register your domain](guides/domains.md) and follow its approval.
4. [Learn what an ACTIVE domain provides](guides/active-domain.md).
5. [Publish events](guides/publishing-events.md), or [subscribe to another domain's events](guides/subscriptions.md).

## Platform administrators: set up Konductor

Follow these pages in order for a new installation:

1. [Plan capacity](operations/service-quotas.md) and raise quotas before onboarding more than three domains.
2. [Configure the platform](operations/configure-platform.md): choose the values that cannot change after setup.
3. [Install Konductor](operations/installation.md) and run its smoke test.
4. [Deploy domain-account trust](operations/trust-stack-deployment.md) into every account that hosts domain resources.
5. [Set up users and access](operations/access-administration.md): Cognito users, team groups, bindings, and entitlements.

After setup, decide registrations in the console's **Pending approvals** inbox;
see the [approval process](guides/domains.md#approval-process). The
[platform administrator guides](operations/README.md) cover recovery, metadata
rollout, schema promotion, system event recovery, and removal.

## Look something up

- [Concepts](architecture/README.md) explain how the platform works and why.
- [Reference](reference/README.md) pages define settings, API contracts, statuses, names, and events.
- The [contributor guide](development/README.md) covers changing the repository.
- The [changelog](../CHANGELOG.md) summarizes notable changes, newest first; it is not a release history.

In the repository, old paths remain as forwarding pages for bookmarks and
archived changes; the site does not publish them. Use the new topic pages for
new links and edits. See [documentation maintenance](development/documentation.md).
