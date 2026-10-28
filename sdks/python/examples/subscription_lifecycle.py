"""Subscribe, inspect, publish, unsubscribe — each a separately invoked step.

Usage: python subscription_lifecycle.py {subscribe,describe,publish,unsubscribe}

Prerequisites (all preconfigured by your platform team, nothing here
provisions them):

- ACTIVE consumer domain (``checkout``) and producer domain (``orders``)
- the schema ``{org}-orders@OrderNotification`` registered and promoted to
  every environment you consume from
- subscriber and publisher bindings for your principal
- an entitlement connecting the two domains
- your own queue and rule for consuming events; the SDK never receives
  events or touches application-side integrations

Receipts are acceptances, not outcomes. Subscribe and unsubscribe are
processed asynchronously, so the steps run separately: describe reports
whatever state exists when you run it, including no record at all yet.
"""

import argparse
import os

from konductor import KonductorClient, ServiceError

CONSUMER_DOMAIN = "checkout"
PRODUCER_DOMAIN = "orders"
SCHEMA_NAME = "acme-orders@OrderNotification"
SUBSCRIPTION_ID = "checkout.orders.OrderNotification"


def subscribe(client):
    receipt = client.subscribe(
        consumer_domain=CONSUMER_DOMAIN,
        producer_domain=PRODUCER_DOMAIN,
        schema_name=SCHEMA_NAME,
    )
    print(f"Subscribe accepted for processing, request {receipt.request_id}")
    print("Acceptance is not activation; run the describe step to observe state.")


def describe(client):
    try:
        detail = client.describe_subscription(subscription_id=SUBSCRIPTION_ID)
    except ServiceError as error:
        if error.status != 404:
            raise
        print("No subscription record yet. A recent subscribe may still be")
        print("processing; run the describe step again later.")
        return
    print(f"Subscription state: {detail.status}")
    for name, env in detail.environments.items():
        line = f"  {name}: {env.status}"
        if env.pinned_version is not None:
            line += f" (schema version {env.pinned_version})"
        if env.error_detail:
            line += f" — failure: {env.error_detail}"
        print(line)


def publish(client):
    receipt = client.publish(
        domain=PRODUCER_DOMAIN,
        event_name="OrderNotification",
        data={"orderId": "ord-123"},
    )
    print(f"Publish accepted for admission, request {receipt.request_id}")


def unsubscribe(client):
    receipt = client.unsubscribe(subscription_id=SUBSCRIPTION_ID)
    print(f"Unsubscribe accepted for processing, request {receipt.request_id}")
    print("Removal is asynchronous; confirm with the describe step later.")
    print("Deleting rules on your own event bus remains your responsibility.")


STEPS = {
    "subscribe": subscribe,
    "describe": describe,
    "publish": publish,
    "unsubscribe": unsubscribe,
}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("step", choices=STEPS)
    args = parser.parse_args(argv)
    with KonductorClient(
        endpoint=os.environ["KONDUCTOR_ENDPOINT"],
        token=os.environ["KONDUCTOR_ACCESS_TOKEN"],
    ) as client:
        STEPS[args.step](client)


if __name__ == "__main__":
    main()
