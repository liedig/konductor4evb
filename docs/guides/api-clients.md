# Insomnia and Postman

[Documentation home](../README.md)

Both clients can sign you in through the Cognito hosted UI with their built-in OAuth 2.0 support, or send an [access token](authentication.md#obtaining-an-access-token) you obtained another way. The [OpenAPI contracts](../reference/api.md) define the routes; no separate endpoint inventory is maintained here.

The OAuth 2.0 settings come from the [stack outputs](../operations/access-administration.md#stack-outputs):

| Setting | Value |
|---|---|
| Authorization URL | `https://<OAUTH_DOMAIN>/oauth2/authorize` |
| Access token URL | `https://<OAUTH_DOMAIN>/oauth2/token` |
| Client ID | The `CognitoUserPoolClientId` output |
| Client secret | Leave empty; the app client is public |
| Callback (redirect) URL | A configured callback URL, `http://localhost:3000/callback` by default |
| Scope | `openid email profile` |

`<OAUTH_DOMAIN>` is the `CognitoOAuthDomain` output without its `https://` prefix. The callback URL must exactly match one of the platform's `oauth_callback_urls` (see [Option A](authentication.md#option-a-hosted-ui-browser)). Both clients capture the redirect themselves, so nothing needs to listen on that address.

## Using Insomnia

To configure requests in Insomnia, complete these steps:

1. **Create a new environment** (or use the base environment) with this variable:

   ```json
   {
     "apiUrl": "https://<API_ID>.execute-api.<REGION>.amazonaws.com/dev"
   }
   ```

2. **Configure authentication** on a folder or the collection so every request inherits it:
   - Type: `OAuth 2.0`
   - Grant type: `Authorization Code`
   - Authorization URL, access token URL, client ID, redirect URL, and scope: from the table above. Leave the client secret empty.
   - Under **Advanced Options**, set **Credentials** to **In Request Body**, because the client has no secret to send in a Basic authorization header.

   Insomnia opens the hosted UI the first time a request needs a token, exchanges the code, and refreshes the access token with the refresh token when it expires.

   To skip the OAuth configuration, choose `Bearer Token` instead and paste an access token. A pasted token stops working after the platform's access-token validity (60 minutes by default).

3. **Create requests** using the variable:
   - `GET {{ apiUrl }}/domains`
   - `POST {{ apiUrl }}/domains` with a JSON body
   - `POST {{ apiUrl }}/domains/registrations/decide` with a JSON body

## Using Postman

To configure a collection in Postman, complete these steps:

1. **Create an environment** with an `apiUrl` variable.

2. **Import the OpenAPI contract**:
   - File > Import > `api/openapi-oauth.yaml`
   - Postman generates all endpoints with the correct paths and methods. Set the collection's base URL variable to `{{apiUrl}}`.

3. **Set up OAuth 2.0 at the collection level**:
   - Collection > Authorization > Type: `OAuth 2.0`
   - Grant Type: `Authorization Code` (`Authorization Code (With PKCE)` also works)
   - Callback URL: `http://localhost:3000/callback`, with **Authorize using browser** cleared
   - Auth URL, Access Token URL, Client ID, and Scope: from the table above. Leave Client Secret empty.
   - Client Authentication: `Send client credentials in body`
   - Choose **Get New Access Token**, sign in through the hosted UI, then choose **Use Token**.

   **Authorize using browser** makes Postman send its own callback URL, `https://oauth.pstmn.io/v1/callback`. Select it only after a platform administrator adds that URL to `oauth_callback_urls`.

   To skip the OAuth configuration, choose `Bearer Token` instead and paste an access token.
