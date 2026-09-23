# Konductor documentation

Konductor for Amazon EventBridge manages event domains and cross-account routing.
Platform teams govern the infrastructure. Application teams publish events and
subscribe to contracts without managing the platform's delivery resources.

Choose a starting point for your task. You do not need to read every section in order.

## Start here

| I want to… | Start with | Continue with |
|---|---|---|
| Understand the system | [Architecture overview](architecture/README.md) | Follow the links for the boundary you need |
| Install or operate Konductor | [Installation](operations/installation.md) | [Trust](operations/trust-stack-deployment.md), [access administration](operations/access-administration.md), [capacity](operations/service-quotas.md) |
| Register a domain | [Authentication](guides/authentication.md) | [Domain guide](guides/domains.md) |
| Publish events or author schemas | [Publishing guide](guides/publishing-events.md) | [Envelope reference](reference/event-envelope.md) |
| Consume another domain's events | [Subscription guide](guides/subscriptions.md) | [Materialization and recovery](architecture/subscriptions.md) |
| Find a route, field, or resource | [Reference index](reference/README.md) | The relevant OpenAPI contract or reference page |
| Change or review the implementation | [Contributor guide](development/README.md) | Relevant requirements, topic docs, and [decisions](decisions/README.md) |
| Understand why a choice was made | [Decision index](decisions/README.md) | Linked OpenSpec design and superseding decisions |
| See what changed | [Changelog](../CHANGELOG.md) | Linked decision or change record |

## How the documentation is organized

- [Guides](guides/README.md): tasks for application teams.
- [Operations](operations/README.md): installation, administration, and runbooks.
- [Architecture](architecture/README.md): system boundaries and design explanations.
- [Reference](reference/README.md): contracts, names, statuses, and inventories.
- [Development](development/README.md): contributor conventions and checks.
- [Decisions](decisions/README.md): rationale, alternatives, constraints, and later changes.
- [History](history/README.md): original proposals and dated snapshots, not current guidance.

## Known documentation conflicts

The reorganization preserves technical content rather than choosing between
conflicting claims. Read the [discrepancy register](reference/documentation-discrepancies.md)
before relying on affected procedures or guarantees. Warnings on the relevant
pages link to individual review items.

Old paths remain as forwarding pages for bookmarks and archived changes. Use
the new topic pages for new links and edits. See [documentation maintenance](development/documentation.md).
