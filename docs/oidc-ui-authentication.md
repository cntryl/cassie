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
  subject. The configured permissions claim is a JSON string array.
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

1. Create a first-party **Regular Web Application** in Auth0. Register the exact
   public Cassie callback URL in **Allowed Callback URLs**, for example
   `https://cassie.example.com/api/v1/auth/oidc/callback`. Enable the
   Authorization Code grant, select **Client Secret (Post)** for client
   authentication, and use RS256 for ID token signing. Keep the client
   secret in the Cassie server's secret configuration.
   [Auth0 application settings](https://auth0.com/docs/get-started/applications/application-settings)
2. Register a dedicated Auth0 API with an identifier such as
   `https://cassie.example.com`. Choose RS256 and enable **RBAC** and
   **Add Permissions in the Access Token**. If the API restricts application
   access, authorize the Cassie application for user access to that API.
   [Auth0 API settings](https://auth0.com/docs/get-started/apis/api-settings) ·
   [Auth0 RBAC settings](https://auth0.com/docs/get-started/apis/enable-role-based-access-control-for-apis) ·
   [Auth0 API access policies](https://auth0.com/docs/get-started/apis/api-access-policies-for-applications)
3. Define an API permission such as `db:analytics`, attach it to an Auth0
   role, and assign that role to the users who should access the database.
   Use `db:*` only for users who should see every current and future database.
   [Auth0 role permissions](https://auth0.com/docs/manage-users/access-control/configure-core-rbac/roles/add-permissions-to-roles)
4. Connect as Cassie `root` over pgwire to the `analytics` database. Create
   an ordinary SQL role and grant the objects its OIDC users need. This
   example assumes `public.reports` exists.

   ```sql
   CREATE ROLE oidc_reader;
   GRANT USAGE ON SCHEMA public TO oidc_reader;
   GRANT SELECT ON TABLE public.reports TO oidc_reader;
   ```

5. Set the Cassie server configuration and restart. Supply
   `CASSIE_OIDC_CLIENT_SECRET` through the deployment's secret store. Keep the
   usual root password and enable HTTPS on Cassie or its trusted TLS edge.
   [Cassie environment settings](environment-variables.md#ui-sign-in-with-openid-connect)

   ```sh
   export CASSIE_OIDC_ISSUER=https://example.us.auth0.com/
   export CASSIE_OIDC_CLIENT_ID=your-auth0-client-id
   export CASSIE_OIDC_API_AUDIENCE=https://cassie.example.com
   export CASSIE_OIDC_REDIRECT_URI=https://cassie.example.com/api/v1/auth/oidc/callback
   export CASSIE_OIDC_ROLE=oidc_reader
   ```

If your provider requires explicit API scope requests, set
`CASSIE_OIDC_SCOPES` to the additional scopes. Cassie always requests
`openid` and reads database grants from the verified access-token
permissions array. Auth0 can include the user's assigned API permissions
in that array even when the token's `scope` claim contains fewer values.
[Auth0 access-token claims](https://auth0.com/docs/get-started/apis/enable-role-based-access-control-for-apis)

Open the Cassie UI in a fresh browser session. It sends you to Auth0 and then
returns to Cassie. An account with `db:analytics` should see **analytics** in
the database picker and be able to read `public.reports` while the SQL grant is
present. Signing out revokes the Cassie session and offers a deliberate
sign-in action. The provider may still have an active session.
