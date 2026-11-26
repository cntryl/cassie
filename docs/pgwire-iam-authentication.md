# AWS IAM workload authentication for pgwire

Cassie uses AWS IAM outbound identity federation. A workload requests a
short-lived token from regional STS using the credentials already available to
its AWS role. The deployment uses a dedicated audience. Cassie service role
grants remain managed through the Cassie database, schema, and table privileges.

The implementation uses a trust file selected with
`CASSIE_PGWIRE_AWS_IAM_TRUST_FILE`. Its JSON object declares the deployment
`audience`, an optional `max_token_lifetime_seconds` (default 300), and
`accounts`. Each account declares a 12-digit `account_id`, its AWS HTTPS
`issuer`, and allowed `algorithms` chosen from `RS256` and `ES384`. Cassie
derives the same-host `/.well-known/jwks.json` endpoint from that issuer.
The verifier requires the ordinary AWS identity subject, its exact IAM role ARN,
the matching AWS account and requesting principal, and one matching audience.
Live AWS qualification is tracked in [#1566](https://github.com/cntryl/cassie/issues/1566).

The pgwire selector is the nonsecret startup username
`cassie_aws_iam`. The token goes in the driver's password field over a
certificate-verified TLS connection. The selector is reserved while IAM
authentication is configured. The session's actual user is the bound Cassie
role. Provision service roles with `CREATE ROLE service EXTERNAL LOGIN`, then
add a binding through `ALTER ROLE service BIND AWS IAM ROLE 'role ARN' ISSUER 'issuer URL'`.
The corresponding `UNBIND` operation removes the exact binding. Grant
database, schema and table privileges as for any other non-admin role.
Dropping the role retires its bindings. Cassie checks the role object ID and
binding identity on later commands, including when a driver resumes a
suspended result. Revocation or replacement does not carry authority into
a different role. Existing password logins retain their password flow.

A pool should acquire a fresh regional STS token when it opens a physical
connection, using a current valid token only until its expiry. Configure
the client to verify the server certificate and hostname (for libpq use
`sslmode=verify-full` and a trusted root). The issuer's key cache has a
five-minute lifetime and limits unknown-key refreshes per account to one
attempt in 15 seconds. Fetches use a two-second timeout, a 64 KiB response
limit, HTTPS to the configured issuer and disabled redirects. Restrict AWS
`sts:GetWebIdentityToken` to the deployment audience, ordinary
`AWSIdentity` subjects and the required lifetime. Enable outbound identity
federation in each issuing account. The server receives role proof and uses
public keys for verification. Its S3 credentials are independent.

[AWS setup](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_providers_outbound_getting_started.html) · [AWS STS API](https://docs.aws.amazon.com/STS/latest/APIReference/API_GetWebIdentityToken.html) · [AWS claims](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_providers_outbound_token_claims.html)
