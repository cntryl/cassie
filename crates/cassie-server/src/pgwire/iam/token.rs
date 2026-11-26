use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;

use super::jwks::{valid_key_id, IssuerKeys};
use super::{AccountTrust, Algorithm, Rejected, TrustSet};

pub(crate) const MAX_TOKEN_BYTES: usize = 16 * 1024;
const CLOCK_SKEW_SECONDS: u64 = 30;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: Algorithm,
    kid: String,
    typ: Option<String>,
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
    aws: AwsClaims,
}

pub(in crate::pgwire) struct VerifiedIdentity {
    pub(in crate::pgwire) issuer: String,
    pub(in crate::pgwire) role_arn: String,
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
        trust: &TrustSet,
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
        if !claims.aud.matches(&trust.audience)
            || claims.aws.aws_account != account.account_id
            || claims.sub != claims.aws.principal_id
            || !aws_role(&claims.sub, &account.account_id)
            || claims
                .subject_mode
                .as_deref()
                .is_some_and(|mode| mode != "AWSIdentity")
            || claims.exp <= now
            || claims.iat > now.saturating_add(CLOCK_SKEW_SECONDS)
            || claims.nbf.is_some_and(|when| when > now)
            || !(1..=trust.max_token_lifetime_seconds)
                .contains(&claims.exp.saturating_sub(claims.iat))
        {
            return Err(Rejected);
        }
        Ok(VerifiedIdentity {
            issuer: claims.iss,
            role_arn: claims.sub,
        })
    }
}
