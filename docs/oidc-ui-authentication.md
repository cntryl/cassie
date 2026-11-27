# OpenID Connect sign-in for the Cassie UI

Cassie can be configured as an OpenID Connect relying party. The UI redirects
an unauthenticated user to the configured provider. Auth0 is the first
integration. The browser receives Cassie's opaque session cookie after the
server completes login.

## Provider and browser contract

- The server accepts one configured HTTPS issuer and obtains its endpoints and
  signing keys through OpenID Connect discovery. The issuer must match the
  discovery document exactly. The callback URI is configured by the operator.
- Login uses Authorization Code with S256 PKCE. A short-lived, single-use
  state is bound to the browser, nonce and PKCE verifier. Callback and return
  paths are fixed or local to Cassie. Provider errors and logout use an
  explicit signed-out screen with a deliberate sign-in action.
- Cassie verifies the signed ID token for its client, including the issuer,
  audience, expiration and nonce. A signed JWT access token for the configured
  Cassie API audience supplies database permissions. Cassie verifies its
  issuer, audience, signature, lifetime and association with the authenticated
  subject. Cassie accepts tokens with an issued lifetime of up to 24 hours.
  The configured permissions claim is a JSON string array.
- Cassie retains only the local authorization and bounded opaque session state.
  OIDC sessions expire by the earlier applicable provider-token expiration
  or the local session limit. OIDC cookies are secure and HTTP-only. Existing
  local password credentials remain available through the password REST API.

## Authorization

The only wildcard is the literal permission `db:*`. It grants access to all
databases on the Cassie instance, including databases created later. A
permission such as `db:analytics` grants access to that database. With no
matching grant, Cassie denies access. The database picker and SQL routes use
the same grants, and the scoped session retains the authorization at SQL
execution. The configured existing non-admin Cassie role supplies schema
and table grants. Its database grants need not duplicate token grants. Cassie
checks the role's identity on requests so dropping, replacing, or promoting it
cannot silently change the external session's authority.

The browser flow uses the Cassie cookie on UI REST requests. The configured
issuer is selected by server configuration. OIDC sessions and outstanding
login challenges are process-local and are cleared by a Cassie restart.

## First use with Auth0

Create an Auth0 application for sign-in and a custom Auth0 API for database
permissions in the same tenant. Auth0 roles group the API permissions you
assign to users. Cassie authorizes each database using the access token's
`permissions` claim.

1. Under **Applications → Applications**, create a first-party **Regular Web
   Application** named `Cassie UI`. In **Settings → Allowed Callback URLs**,
   enter `https://cassie.example.com/api/v1/auth/oidc/callback` with your
   public Cassie hostname. Use the **Authorization Code** grant and RS256 ID
   token signing in **Advanced Settings**. Under **Credentials**, select
   **Client Secret (Post)**. Enable the users' sign-in connection under
   **Connections**. Keep the application secret in the Cassie server's
   secret configuration.
   [Auth0 application settings](https://auth0.com/docs/get-started/applications/application-settings)
2. Under **Applications → APIs**, create an API named `Cassie API`. Set its
   **Identifier** to `https://cassie.example.com` or another dedicated audience
   and choose **RS256**. In the API's **Settings → RBAC Settings**, turn on
   **Enable RBAC** and **Add Permissions in the Access Token**. Add
   `db:analytics` under the API's **Permissions** tab. If **User-Delegated
   Access** is set to **Per-app authorization**, open the API's **Application
   Access** tab, edit `Cassie UI`, and grant it user-delegated access with the
   applicable Cassie permissions.
   [Auth0 API settings](https://auth0.com/docs/get-started/apis/api-settings) ·
   [Auth0 RBAC settings](https://auth0.com/docs/get-started/apis/enable-role-based-access-control-for-apis) ·
   [Auth0 API access policies](https://auth0.com/docs/get-started/apis/api-access-policies-for-applications)
3. Under **User Management → Roles**, create `Cassie Analytics Reader`. Open
   the role's **Permissions** tab, choose `Cassie API`, and add `db:analytics`.
   Under **User Management → Users**, open a test user and assign the role
   from the **Roles** tab. For a user who should see every current and future
   database, add the literal `db:*` permission to the API and assign it via
   a separate role.
   [Auth0 role permissions](https://auth0.com/docs/manage-users/access-control/configure-core-rbac/roles/add-permissions-to-roles)
   · [Auth0 user role assignments](https://auth0.com/docs/manage-users/access-control/configure-core-rbac/rbac-users/assign-roles-to-users)
4. Connect as Cassie `root` over pgwire to the `analytics` database. Create
   an ordinary SQL role and grant the objects its OIDC users need. This
   example assumes `public.reports` exists.

   ```sql
   CREATE ROLE oidc_reader;
   GRANT USAGE ON SCHEMA public TO oidc_reader;
   GRANT SELECT ON TABLE public.reports TO oidc_reader;
   ```

5. Set the Cassie server configuration and restart. Use the application's
   **Domain** (as an HTTPS issuer with its trailing slash) and **Client ID**.
   Supply its **Client Secret** as `CASSIE_OIDC_CLIENT_SECRET` through the
   deployment's secret store. Match the audience to the API **Identifier**
   and the redirect URI to the **Allowed Callback URLs** entry. Keep the usual
   root password and enable HTTPS on Cassie or its trusted TLS edge.
   [Cassie environment settings](environment-variables.md#ui-sign-in-with-openid-connect)

   ```sh
   export CASSIE_OIDC_ISSUER=https://example.us.auth0.com/
   export CASSIE_OIDC_CLIENT_ID=your-auth0-client-id
   export CASSIE_OIDC_API_AUDIENCE=https://cassie.example.com
   export CASSIE_OIDC_REDIRECT_URI=https://cassie.example.com/api/v1/auth/oidc/callback
   export CASSIE_OIDC_CLIENT_AUTH_METHOD=client_secret_post
   export CASSIE_OIDC_ROLE=oidc_reader
   ```

If your provider requires explicit API scope requests, set
`CASSIE_OIDC_SCOPES` to the additional scopes. With Auth0 per-application
API access, request the required API scopes explicitly, for example
`CASSIE_OIDC_SCOPES=db:analytics`. Cassie always requests `openid` and reads
database grants from the verified access-token permissions array. Auth0 can
include the user's assigned API permissions in that array even when the token's
`scope` claim contains fewer values.
[Auth0 access-token claims](https://auth0.com/docs/get-started/apis/enable-role-based-access-control-for-apis)

Open the Cassie UI in a fresh browser session. It sends you to Auth0 and then
returns to Cassie. An account with `db:analytics` should see **analytics** in
the database picker and be able to read `public.reports` while the SQL grant is
present. If Auth0 signs you in and Cassie rejects the login or omits the
expected database, check that the user's role includes its exact permission
on the API configured as the audience and that **Add Permissions in the
Access Token** is enabled on that API. Sign in again after changing the
user's permissions to start a new Cassie session. Signing out revokes the
Cassie session and offers a deliberate sign-in action. The provider may still
have an active session.
