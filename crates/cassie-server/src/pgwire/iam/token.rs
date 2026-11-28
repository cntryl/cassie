use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;

use super::jwks::{valid_key_id, IssuerKeys};
use super::{
    valid_google_subject, valid_guid, AccountTrust, Algorithm, AzureVersion, Provider, Rejected,
};

pub(crate) const MAX_TOKEN_BYTES: usize = 16 * 1024;
const CLOCK_SKEW_SECONDS: u64 = 30;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: Algorithm,
    kid: String,
    typ: Option<String>,
    #[serde(rename = "x5t")]
    _x5t: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    List(Vec<String>),
}

impl Audience {
    fn matches(&self, expected: &str) -> bool {
        match self {
            Self::One(value) => value == expected,
            Self::List(values) => values.len() == 1 && values[0] == expected,
        }
    }
}

#[derive(Deserialize)]
struct AwsClaims {
    aws_account: String,
    principal_id: String,
    subject_mode: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: Audience,
    sub: String,
    iat: u64,
    exp: u64,
    nbf: Option<u64>,
    subject_mode: Option<String>,
    #[serde(rename = "https://sts.amazonaws.com/")]
    aws: Option<AwsClaims>,
    tid: Option<String>,
    oid: Option<String>,
    idtyp: Option<String>,
    ver: Option<String>,
    azp: Option<String>,
    appid: Option<String>,
    scp: Option<String>,
}

pub(in crate::pgwire) struct VerifiedIdentity {
    pub(in crate::pgwire) issuer: String,
    pub(in crate::pgwire) role_arn: String,
    pub(in crate::pgwire) is_aws: bool,
}

pub(super) struct Token<'a> {
    signing_input: &'a str,
    signature: Vec<u8>,
    header: Header,
    claims: Claims,
}

fn aws_role(arn: &str, account: &str) -> bool {
    let mut fields = arn.splitn(6, ':');
    if fields.next() != Some("arn")
        || fields.next() != Some("aws")
        || fields.next() != Some("iam")
        || fields.next() != Some("")
        || fields.next() != Some(account)
    {
        return false;
    }
    fields
        .next()
        .and_then(|resource| resource.strip_prefix("role/"))
        .is_some_and(|name| {
            !name.is_empty() && !name.ends_with('/') && name.bytes().all(|b| b.is_ascii_graphic())
        })
}

impl<'a> Token<'a> {
    pub(super) fn parse(raw: &'a str) -> Result<Self, Rejected> {
        if raw.len() > MAX_TOKEN_BYTES {
            return Err(Rejected);
        }
        let (signing_input, signature) = raw.rsplit_once('.').ok_or(Rejected)?;
        let (header, claims) = signing_input.split_once('.').ok_or(Rejected)?;
        let bytes = URL_SAFE_NO_PAD.decode(header).map_err(|_| Rejected)?;
        let header: Header = serde_json::from_slice(&bytes).map_err(|_| Rejected)?;
        if !valid_key_id(&header.kid) || header.typ.as_deref().is_some_and(|typ| typ != "JWT") {
            return Err(Rejected);
        }
        let bytes = URL_SAFE_NO_PAD.decode(claims).map_err(|_| Rejected)?;
        let claims = serde_json::from_slice(&bytes).map_err(|_| Rejected)?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| Rejected)?;
        Ok(Self {
            signing_input,
            signature,
            header,
            claims,
        })
    }

    pub(super) fn issuer(&self) -> &str {
        &self.claims.iss
    }

    pub(super) fn algorithm(&self) -> Algorithm {
        self.header.alg
    }

    pub(super) fn kid(&self) -> &str {
        &self.header.kid
    }

    pub(super) fn verify(
        self,
        account: &AccountTrust,
        keys: &IssuerKeys,
        now: u64,
    ) -> Result<VerifiedIdentity, Rejected> {
        keys.verify(
            self.kid(),
            self.algorithm(),
            self.signing_input.as_bytes(),
            &self.signature,
        )?;
        let claims = self.claims;
        if !claims.aud.matches(&account.audience)
            || claims.exp <= now
            || claims.iat > now.saturating_add(CLOCK_SKEW_SECONDS)
            || claims.nbf.is_some_and(|when| when > now)
            || !(1..=account.max_token_lifetime_seconds)
                .contains(&claims.exp.saturating_sub(claims.iat))
        {
            return Err(Rejected);
        }
        let is_aws = matches!(account.provider, Provider::Aws { .. });
        let identity = match &account.provider {
            Provider::Aws { account_id } => claims.aws_subject(account_id)?,
            Provider::Azure { tenant_id, version } => claims.azure_subject(tenant_id, *version)?,
            Provider::Google => claims.google_subject()?,
        }
        .to_owned();
        Ok(VerifiedIdentity {
            issuer: claims.iss,
            role_arn: identity,
            is_aws,
        })
    }
}

impl Claims {
    fn aws_subject(&self, account: &str) -> Result<&str, Rejected> {
        let aws = self.aws.as_ref().ok_or(Rejected)?;
        if aws.aws_account != account
            || self.sub != aws.principal_id
            || !aws_role(&self.sub, account)
            || [self.subject_mode.as_deref(), aws.subject_mode.as_deref()]
                .into_iter()
                .flatten()
                .any(|mode| mode != "AWSIdentity")
        {
            return Err(Rejected);
        }
        Ok(&self.sub)
    }

    fn azure_subject(&self, tenant: &str, version: AzureVersion) -> Result<&str, Rejected> {
        let (version, actor) = match version {
            AzureVersion::V1 => ("1.0", self.appid.as_deref()),
            AzureVersion::V2 => ("2.0", self.azp.as_deref()),
        };
        if self.tid.as_deref() != Some(tenant)
            || self.ver.as_deref() != Some(version)
            || self.idtyp.as_deref() != Some("app")
            || self.scp.is_some()
            || !actor.is_some_and(valid_guid)
        {
            return Err(Rejected);
        }
        self.oid
            .as_deref()
            .filter(|id| valid_guid(id))
            .ok_or(Rejected)
    }

    fn google_subject(&self) -> Result<&str, Rejected> {
        if !valid_google_subject(&self.sub) || self.azp.as_deref() != Some(self.sub.as_str()) {
            return Err(Rejected);
        }
        Ok(&self.sub)
    }
}
