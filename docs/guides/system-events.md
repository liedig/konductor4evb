# Consume system events

[Documentation home](../README.md)

Konductor publishes platform-to-domain **system events**—starting with
subscription upgrade notifications—to a platform-owned SNS topic per domain.
This guide shows how a domain team subscribes an SQS queue or Lambda function
and consumes events safely. The payload contract is documented in the
[system events reference](../reference/system-events.md); operational failure
recovery is a platform concern covered by
[system event recovery](../operations/system-events.md).

## Prerequisites

- Your domain has a system-events topic. Find the topic ARN on the
  **System events** page in the console (the header shows a dot when new
  events have been delivered since your last visit), or via the domain
  detail API as `systemEventsTopicArn`.
  Domains registered before system events get their topic from the platform
  backfill—ask your platform administrator if the ARN is missing.
- You subscribe from one of the domain's **registered accounts**: the owner
  account or an effective environment account on the domain record.

## Subscribe an SQS queue

The topic policy permits `sns:Subscribe` from your domain's registered
accounts for **SQS, Lambda, and email endpoints**. SQS and Lambda endpoint
ARNs must live in those same accounts; email addresses cannot be constrained
that way (see below). HTTPS, SMS, and other protocols are not permitted, and
domain principals cannot publish to or administer the topic.

Create the queue in a registered account and scope its resource policy to the
topic as the only allowed source:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "AllowSystemEventsTopic",
      "Effect": "Allow",
      "Principal": { "Service": "sns.amazonaws.com" },
      "Action": "sqs:SendMessage",
      "Resource": "arn:aws:sqs:eu-central-1:111122223333:checkout-system-events",
      "Condition": {
        "ArnEquals": {
          "aws:SourceArn": "arn:aws:sns:eu-central-1:123456789012:acme-checkout-system-events"
        }
      }
    }
  ]
}
```

Then subscribe the queue:

```bash
aws sns subscribe \
  --topic-arn arn:aws:sns:eu-central-1:123456789012:acme-checkout-system-events \
  --protocol sqs \
  --notification-endpoint arn:aws:sqs:eu-central-1:111122223333:checkout-system-events
```

**You own the subscription.** Keep the returned subscription ARN, and attach
your own DLQ (redrive policy) to the queue: endpoint delivery failures are the
endpoint owner's responsibility, and the platform does not redrive them.

## Email subscriptions

Email is a permitted protocol alongside SQS and Lambda. Because email
addresses are not ARNs, the topic policy cannot restrict which addresses are
subscribed. Anyone with credentials in a registered account can subscribe an
address, so treat mailbox membership as the access boundary for these
low-sensitivity version notifications.

**The domain owner is subscribed by default.** Provisioning (and the
existing-domain backfill) subscribes the domain record's `ownerEmail` to the
topic. SNS email subscriptions start *pending confirmation*: the address
owner must click the confirmation link in the SNS email before delivery
begins, and an unconfirmed subscription expires after a few days. When the
domain's owner email changes, the platform moves the default subscription to
the new address (the new owner confirms again; a still-pending old
subscription is left to expire).

To subscribe another address yourself:

```bash
aws sns subscribe \
  --topic-arn arn:aws:sns:eu-central-1:123456789012:acme-checkout-system-events \
  --protocol email \
  --notification-endpoint alerts@example.com
```

For anything automated, prefer SQS or Lambda — email is for humans, and the
same filtering attributes apply to every protocol.

## Subscribe a Lambda function

Grant SNS invocation scoped to the topic, then subscribe the function:

```bash
aws lambda add-permission \
  --function-name checkout-system-events \
  --statement-id konductor-system-events \
  --action lambda:InvokeFunction \
  --principal sns.amazonaws.com \
  --source-arn arn:aws:sns:eu-central-1:123456789012:acme-checkout-system-events

aws sns subscribe \
  --topic-arn arn:aws:sns:eu-central-1:123456789012:acme-checkout-system-events \
  --protocol lambda \
  --notification-endpoint arn:aws:lambda:eu-central-1:111122223333:function:checkout-system-events
```

## The SNS envelope and raw delivery

By default SNS wraps the event: the body your endpoint receives is a
notification JSON document whose `Message` field is a *string* containing the
event JSON. Set `RawMessageDelivery: true` on the subscription to receive the
event JSON as the body directly:

```bash
aws sns set-subscription-attributes \
  --subscription-arn <subscription-arn> \
  --attribute-name RawMessageDelivery \
  --attribute-value true
```

Either way, the message content is the full EventBridge event
(`version`, `id`, `detail-type`, `source`, `account`, `time`, `region`,
`detail`). Parse `detail`; the transport `id` is routing metadata, not part of
the event's identity contract.

## Filter policies

Every message carries filter attributes that agree with the body by
construction. Filter at the subscription so your endpoint only sees what it
acts on:

| Attribute | Values |
|-----------|--------|
| `eventType` | `SubscriptionUpgradeStarted`, `SubscriptionUpgraded` |
| `schemaVersion` | Numeric public contract version (currently `1`) |
| `audience` | `CONSUMER` |
| `domain` | The recipient domain |
| `environment` | The subscription's environment, or `ALL` |
| `resourceType` | `SUBSCRIPTION` |
| `trigger` | `PROMOTION` or `AUTHORING` |
| `outcome` | Terminal events only: `COMPLETED` or `PARTIAL` |

Example—only completed production promotions:

```json
{
  "eventType": ["SubscriptionUpgraded"],
  "trigger": ["PROMOTION"],
  "environment": ["prod"],
  "outcome": ["COMPLETED"]
}
```

```bash
aws sns set-subscription-attributes \
  --subscription-arn <subscription-arn> \
  --attribute-name FilterPolicy \
  --attribute-value '{"eventType":["SubscriptionUpgraded"],"trigger":["PROMOTION"],"environment":["prod"],"outcome":["COMPLETED"]}'
```

A filter attribute that is absent from a message never matches a policy that
names it, so a start event never matches an `outcome` filter.

## Consume safely

- **Deduplicate** on `detail.metadata.idempotencyKey` before any external
  action. Delivery is at-least-once; retries and redeliveries carry the same
  key. There is no exactly-once delivery.
- **Order by `detail.metadata.resourceRevision`** per subscription and
  environment; discard revisions at or below one you already applied.
- A start event is **not guaranteed a matching terminal event**: a newer
  operation can supersede an in-flight one. Never reconcile by counting
  start/terminal pairs; track the highest applied revision and read current
  state when in doubt.
- Terminal events tell the truth: `outcome` is `PARTIAL` (with a safe
  `reason` classification) when the resulting version is below the requested
  one, such as a replay that stopped early.
- **Initialize from current state.** When you start consuming, read the
  current pins through the subscriptions API (`GET /subscriptions`) rather
  than expecting history; events before your subscription are not replayed.
- **Fetch exact contracts** with the event's `contracts` selectors
  (`producerDomain`, `schemaName`, `environment`, `schemaVersion`,
  `metadataRevision`) through the existing contract API
  (`GET /schemas/{schemaName}/contract`). That route requires a publisher
  binding on the producer domain; consumers without one obtain the composed
  contract from the producer team. Event receipt grants no new access.

## Durability and failure handling

The platform projects committed state onto the system bus with bounded
retries and retains undeliverable projections for operator recovery, so
short platform-side outages do not lose events. SNS retries endpoint
deliveries on its own schedule, and undeliverable messages land on your
subscription's DLQ if you configured one.

The 90-day EventBridge archive on the system bus is **audit-only**: there is
no archive replay and no historical catch-up in this release. If your
endpoint was down long enough to lose messages, reconcile from the current
state APIs instead of asking for a replay.

If your platform enables SES notifications, domain owners also receive
advisory emails for promotion-triggered upgrades. Emails can be delayed,
duplicated, or out of order and are never replayed — always treat the SNS
stream as authoritative. See [Optional owner emails](../reference/system-events.md#optional-owner-emails).
