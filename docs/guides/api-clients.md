# Configure Insomnia or Postman

[Documentation home](../README.md)

Obtain an [access token](authentication.md) first. The [OpenAPI contracts](../reference/api.md) define the routes; no separate endpoint inventory is maintained here.

> **Review pending:** [D07](../reference/documentation-discrepancies.md#d07) records conflicting or unverified guidance retained from the previous documentation. This reorganization does not resolve it.

## Using Insomnia

To configure requests in Insomnia, complete these steps:

1. **Create a new environment** (or use the base environment) with these variables:

   ```json
   {
     "apiUrl": "https://<API_ID>.execute-api.<REGION>.amazonaws.com/dev",
     "accessToken": "PASTE_YOUR_ACCESS_TOKEN_HERE"
   }
   ```

2. **Add a Bearer token to all requests**: Create a sub-environment or folder-level auth:
   - Type: `Bearer Token`
   - Token: `{{ accessToken }}`

3. **Create requests** using the variable:
   - `GET {{ apiUrl }}/domains`
   - `POST {{ apiUrl }}/domains` with a JSON body
   - `POST {{ apiUrl }}/domains/registrations/decide` with a JSON body

4. **(Optional) Automate token retrieval**: Insomnia supports pre-request scripts. You can add a script that calls the Cognito token endpoint and stores the result:

   ```javascript
   // In the request "Before Send Script" tab:
   const response = await insomnia.sendRequest({
     url: 'https://<OAUTH_DOMAIN>/oauth2/token',
     method: 'POST',
     headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
     body: {
       grant_type: 'authorization_code',
       client_id: '<CLIENT_ID>',
       code: pm.environment.get('authCode'),
       redirect_uri: 'http://localhost'
     }
   });
   const body = JSON.parse(response.body);
   insomnia.set('accessToken', body.access_token);
   ```

   For the admin auth flow (no browser), use a pre-request script that calls `aws cognito-idp admin-initiate-auth` via a shell command, or set the token manually after retrieving it with the AWS CLI.

## Using Postman

To configure a collection in Postman, complete these steps:

1. **Create an environment** with `apiUrl` and `accessToken` variables.

2. **Set up authorization at the collection level**:
   - Type: `Bearer Token`
   - Token: `{{accessToken}}`

3. **Import the OpenAPI contract**:
   - File > Import > `api/openapi-oauth.yaml`
   - Postman generates all endpoints with the correct paths and methods.

4. **(Optional) OAuth 2.0 flow automation**:
   - Collection > Authorization > Type: `OAuth 2.0`
   - Configure:
     - Callback URL: `http://localhost`
     - Auth URL: `https://<OAUTH_DOMAIN>/oauth2/authorize`
     - Access Token URL: `https://<OAUTH_DOMAIN>/oauth2/token`
     - Client ID: `<CLIENT_ID>`
     - Scope: `openid email profile`
     - Grant Type: `Authorization Code`
   - Choose **Get New Access Token** to open the browser, sign in, and exchange the code automatically.
