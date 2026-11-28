# Cloud workload authentication for pgwire

## AWS IAM

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
The selected issuer format covers commercial AWS account endpoints under
`tokens.sts.global.api.aws`. The server requires a pgwire TLS certificate and
key plus the existing root password before enabling IAM. Live AWS
qualification is tracked in [#1567](https://github.com/cntryl/cassie/issues/1567).

The pgwire selector is the nonsecret startup username
`cassie_aws_iam`. The token goes in the driver's password field over a
certificate-verified TLS connection and specifies the target database
explicitly. The selector is reserved while IAM
authentication is configured. The session's actual user is the bound Cassie
role. Provision service roles with `CREATE ROLE service EXTERNAL LOGIN`, then
add a binding through `ALTER ROLE service BIND AWS IAM ROLE 'role ARN' ISSUER 'issuer URL'`.
The corresponding `UNBIND` operation removes the exact binding. Grant
database, schema and table privileges as for any other non-admin role.
Dropping the role retires its bindings. Cassie checks the role object ID and
binding identity on later commands, including when a driver resumes a
suspended result. Revocation or replacement does not carry authority into
a different role. Removing the binding also releases the suspended portal.
An ordinary table grant change applies to newly admitted queries. Resuming an
already materialized result follows the original statement admission and
snapshot. Existing password logins retain their password flow.

On Fargate, assign the calling application task an IAM task role authorized to
request the token. The ECS task execution role serves the container platform,
and the Cassie task storage role grants its storage access. Allow the
application task to reach regional STS and the Cassie task to reach the
configured issuer JWKS endpoint. Connect the application to the Cassie service
DNS name covered by the pgwire TLS certificate.

A pool should acquire a fresh regional STS token when it opens a physical
connection, using a current valid token only until its expiry. Configure
the client to verify the server certificate and hostname (for libpq use
`sslmode=verify-full` and a trusted root). The issuer's key cache has a
five-minute lifetime and limits unknown-key refreshes per account to one
attempt in 15 seconds. Fetches use a two-second timeout, a 64 KiB response
limit, at most 32 keys, HTTPS to the configured issuer and disabled redirects. Restrict AWS
`sts:GetWebIdentityToken` to the deployment audience, ordinary
`AWSIdentity` subjects and the required lifetime. Enable outbound identity
federation in each issuing account. The server receives role proof and uses
public keys for verification. Its S3 credentials are independent.

[AWS setup](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_providers_outbound_getting_started.html) · [AWS STS API](https://docs.aws.amazon.com/STS/latest/APIReference/API_GetWebIdentityToken.html) · [AWS claims](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_roles_providers_outbound_token_claims.html)

## Azure and Google Cloud workload identity

Set `CASSIE_PGWIRE_WORKLOAD_TRUST_FILE` to a JSON trust file to enable
`cassie_workload`. Choose one trust source per server. The new file
can include the AWS `audience` and `accounts` settings alongside Azure and Google
Cloud. AWS clients can use either selector when their account is configured in
the workload trust file. Each provider verifies its own configured audience.

An Azure and Google Cloud trust file can look like this.

```json
{
  "azure": [
    {
      "tenant_id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
      "token_version": "2.0",
      "audience": "00000000-1111-2222-3333-444444444444"
    }
  ],
  "google": {"audience": "https://cassie.example/google"}
}
```

For Azure, register a Cassie resource application in Microsoft Entra ID.
Configure its access-token version and the `idtyp` optional access-token claim
so managed identity tokens carry `idtyp=app`. Request a token for the registered
resource using the platform's managed identity credential. In the trust file,
set `audience` to the exact access-token audience, the resource application's
client ID for v2 tokens and its resource application ID URI for the illustrated
v1 form. The issuer is
`https://login.microsoftonline.com/<tenant_id>/v2.0` for v2 tokens or
`https://sts.windows.net/<tenant_id>/` for v1 tokens. Use the managed identity's
tenant-local object ID as the binding subject.

For Google Cloud, request a Google-signed ID token for the configured audience
from the attached service account using the workload platform's metadata server.
Use the service account's numeric unique ID as the binding subject and
`https://accounts.google.com` as the issuer.

Create an external login and bind the exact subject and issuer to it. For example
these statements grant distinct application identities their own Cassie roles.

```sql
CREATE ROLE azure_service EXTERNAL LOGIN;
ALTER ROLE azure_service BIND WORKLOAD IDENTITY 'ffffffff-bbbb-cccc-dddd-aaaaaaaaaaaa'
  ISSUER 'https://login.microsoftonline.com/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/v2.0';

CREATE ROLE google_service EXTERNAL LOGIN;
ALTER ROLE google_service BIND WORKLOAD IDENTITY '112010400000000710080'
  ISSUER 'https://accounts.google.com';
```

The same `BIND WORKLOAD IDENTITY` form also accepts an AWS IAM role ARN and its
issuer. AWS clients can continue using existing `BIND AWS IAM ROLE` bindings.
Grant each Cassie role its required privileges and connect over certificate-
verified TLS with the target database, startup username `cassie_workload`, and
the short-lived token in the password field. Run `SELECT current_user;` to verify
the mapping. Use `UNBIND WORKLOAD IDENTITY` with the same subject and issuer to
revoke the binding. Cassie then denies subsequent statements under that binding.

## Flat environment settings

For simple deployments, configure one AWS account, one Azure tenant, and/or
one Google Cloud audience without a JSON file. Leave both trust-file variables
unset. Every configured provider uses the same pgwire TLS requirements and SQL
role bindings shown above. For example, these variables enable all three
providers for a single server.

```sh
export CASSIE_PGWIRE_AWS_IAM_ACCOUNT_ID=123456789012
export CASSIE_PGWIRE_AWS_IAM_ISSUER=https://example.tokens.sts.global.api.aws
export CASSIE_PGWIRE_AWS_IAM_AUDIENCE=https://cassie.example/aws
export CASSIE_PGWIRE_AZURE_TENANT_ID=aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee
export CASSIE_PGWIRE_AZURE_AUDIENCE=00000000-1111-2222-3333-444444444444
export CASSIE_PGWIRE_GOOGLE_AUDIENCE=https://cassie.example/google
```

Set `CASSIE_PGWIRE_AZURE_TOKEN_VERSION=1.0` for v1 access tokens. Azure defaults
to v2. AWS permits `RS256,ES384` and a 300-second maximum lifetime by default.
Use `CASSIE_PGWIRE_AWS_IAM_ALGORITHMS` and
`CASSIE_PGWIRE_AWS_IAM_MAX_TOKEN_LIFETIME_SECONDS` to restrict that trust.
Use the JSON workload trust file for multiple AWS accounts or Azure tenants.

[Microsoft Entra claims validation](https://learn.microsoft.com/en-us/entra/identity-platform/claims-validation) · [Google Cloud service account ID tokens](https://cloud.google.com/docs/authentication/token-types#service-account-id-tokens)
