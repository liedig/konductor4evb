"""Publish with access tokens refreshed from a Cognito refresh token.

Suits long-running processes: the provider exchanges the refresh token for
an access token, reuses it until shortly before it expires, then exchanges
again. Get a refresh token by signing in through the hosted UI (see the
authentication guide); it stays valid for 30 days.

Run with:  KONDUCTOR_ENDPOINT=https://{api-id}.execute-api.{region}.amazonaws.com/prod \
           KONDUCTOR_OAUTH_DOMAIN={CognitoOAuthDomain} \
           KONDUCTOR_CLIENT_ID={CognitoUserPoolClientId} \
           KONDUCTOR_REFRESH_TOKEN=... python publish_with_refresh_token.py
"""

import os

from konductor import CognitoRefreshTokenProvider, KonductorClient


def token_provider_from_environment():
    return CognitoRefreshTokenProvider(
        oauth_domain=os.environ["KONDUCTOR_OAUTH_DOMAIN"],
        client_id=os.environ["KONDUCTOR_CLIENT_ID"],
        refresh_token=os.environ["KONDUCTOR_REFRESH_TOKEN"],
    )


def run(client, *, domain, event_name, data):
    receipt = client.publish(domain=domain, event_name=event_name, data=data)
    print(f"Accepted for admission, request {receipt.request_id}")
    return receipt


def main():
    with KonductorClient(
        endpoint=os.environ["KONDUCTOR_ENDPOINT"],
        token_provider=token_provider_from_environment(),
    ) as client:
        for order in ("ord-1", "ord-2", "ord-3"):
            run(
                client,
                domain="orders",
                event_name="OrderNotification",
                data={"orderId": order},
            )


if __name__ == "__main__":
    main()
