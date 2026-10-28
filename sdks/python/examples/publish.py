"""Publish one event with a Cognito access token.

Prerequisites: an ACTIVE producer domain, a registered schema for the event
in the endpoint's environment, and a publisher binding for your team group.
Obtain an access token through the flows in the authentication guide; this
example reads one from KONDUCTOR_ACCESS_TOKEN and provisions nothing.

Run with:  KONDUCTOR_ENDPOINT=https://{api-id}.execute-api.{region}.amazonaws.com/prod \
           KONDUCTOR_ACCESS_TOKEN=... python publish.py
"""

import os

from konductor import KonductorClient


def run(client, *, domain, event_name, data):
    receipt = client.publish(domain=domain, event_name=event_name, data=data)
    print(f"Accepted for admission, request {receipt.request_id}")
    print("Acceptance means validation passed and the event is queued;")
    print("delivery to subscribers is asynchronous.")
    return receipt


def main():
    with KonductorClient(
        endpoint=os.environ["KONDUCTOR_ENDPOINT"],
        token=os.environ["KONDUCTOR_ACCESS_TOKEN"],
    ) as client:
        run(
            client,
            domain="orders",
            event_name="OrderNotification",
            data={"orderId": "ord-123", "total": 42.50},
        )


if __name__ == "__main__":
    main()
