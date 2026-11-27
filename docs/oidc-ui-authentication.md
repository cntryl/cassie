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
