//! Verify the signed identity and API tokens independently against the issuer keys.

use std::collections::BTreeMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ring::digest::{digest, SHA256};
use ring::signature::{RsaPublicKeyComponents, RSA_PKCS1_2048_8192_SHA256};
use serde::Deserialize;
use serde_json::Value;

use super::{Rejected, Settings};

const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_TOKEN_LIFETIME: u64 = 24 * 60 * 60;
const CLOCK_SKEW: u64 = 60;

#[derive(Deserialize)]
pub(super) struct TokenPair {
    pub id_token: String,
    pub access_token: String,
}

pub(super) struct Verified {
    pub subject: String,
    pub permissions: Vec<String>,
    pub expires_at: u64,
}

#[derive(Deserialize)]
struct IssuerKeys {
    keys: Vec<Key>,
}

#[derive(Deserialize)]
struct Key {
    kid: Option<String>,
    kty: String,
    alg: Option<String>,
    #[serde(rename = "use")]
    usage: Option<String>,
    key_ops: Option<Vec<String>>,
    n: Option<String>,
    e: Option<String>,
}

impl Key {
    fn verify(&self, input: &str, signature: &str) -> Result<(), Rejected> {
        if self.kty != "RSA"
            || self.alg.as_deref().is_some_and(|alg| alg != "RS256")
            || self.usage.as_deref().is_some_and(|usage| usage != "sig")
            || self
                .key_ops
                .as_ref()
                .is_some_and(|ops| !ops.iter().any(|op| op == "verify"))
        {
            return Err(Rejected);
        }
        let n = decode(self.n.as_deref().ok_or(Rejected)?)?;
        let e = decode(self.e.as_deref().ok_or(Rejected)?)?;
        if !(256..=1024).contains(&n.len()) || !(1..=5).contains(&e.len()) {
            return Err(Rejected);
        }
        RsaPublicKeyComponents { n: &n, e: &e }
            .verify(
                &RSA_PKCS1_2048_8192_SHA256,
                input.as_bytes(),
                &decode(signature)?,
            )
            .map_err(|_| Rejected)
    }
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
    #[serde(default)]
    crit: Option<Value>,
    #[serde(default)]
    b64: Option<Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    Single(String),
    Multiple(Vec<String>),
}

impl Audience {
    fn contains(&self, audience: &str) -> bool {
        match self {
            Self::Single(single) => single == audience,
            Self::Multiple(values) => values.iter().any(|value| value == audience),
        }
    }

    fn only(&self, audience: &str) -> bool {
        match self {
            Self::Single(single) => single == audience,
            Self::Multiple(values) => matches!(values.as_slice(), [one] if one == audience),
        }
    }
}

#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: Audience,
    exp: u64,
    iat: u64,
    nbf: Option<u64>,
    azp: Option<String>,
    nonce: Option<String>,
    at_hash: Option<String>,
    #[serde(flatten)]
    additional: BTreeMap<String, Value>,
}

fn decode(segment: &str) -> Result<Vec<u8>, Rejected> {
    URL_SAFE_NO_PAD.decode(segment).map_err(|_| Rejected)
}

fn claims(
    token: &str,
    keys: &IssuerKeys,
    settings: &Settings,
    now: u64,
) -> Result<Claims, Rejected> {
    if token.len() > MAX_TOKEN_BYTES {
        return Err(Rejected);
    }
    let (input, signature) = token.rsplit_once('.').ok_or(Rejected)?;
    let (header, payload) = input.split_once('.').ok_or(Rejected)?;
    let header: Header = serde_json::from_slice(&decode(header)?).map_err(|_| Rejected)?;
    if header.alg != "RS256"
        || header.kid.is_empty()
        || header.kid.len() > 128
        || !header.kid.bytes().all(|byte| byte.is_ascii_graphic())
        || header.crit.is_some()
        || header.b64.is_some()
    {
        return Err(Rejected);
    }
    let mut matching = keys
        .keys
        .iter()
        .filter(|key| key.kid.as_ref() == Some(&header.kid));
    let key = matching.next().ok_or(Rejected)?;
    if matching.next().is_some() {
        return Err(Rejected);
    }
    key.verify(input, signature)?;
    let claims: Claims = serde_json::from_slice(&decode(payload)?).map_err(|_| Rejected)?;
    if claims.iss != settings.issuer
        || claims.sub.is_empty()
        || claims.sub.len() > 255
        || claims.exp <= now
        || claims.iat > now.saturating_add(CLOCK_SKEW)
        || claims.exp <= claims.iat
        || claims.exp - claims.iat > MAX_TOKEN_LIFETIME
        || claims.nbf.is_some_and(|nbf| nbf > now)
        || claims
            .azp
            .as_deref()
            .is_some_and(|azp| azp != settings.client_id)
    {
        return Err(Rejected);
    }
    Ok(claims)
}

pub(super) fn verify(
    settings: &Settings,
    keys: &[u8],
    pair: &TokenPair,
    nonce: &str,
    now: u64,
) -> Result<Verified, Rejected> {
    let keys: IssuerKeys = serde_json::from_slice(keys).map_err(|_| Rejected)?;
    if keys.keys.is_empty() || keys.keys.len() > 32 {
        return Err(Rejected);
    }
    let id = claims(&pair.id_token, &keys, settings, now)?;
    let api = claims(&pair.access_token, &keys, settings, now)?;
    if !id.aud.only(&settings.client_id)
        || id.nonce.as_deref() != Some(nonce)
        || !api.aud.contains(&settings.audience)
        || api.aud.contains(&settings.client_id)
        || api.sub != id.sub
        || api
            .additional
            .get("client_id")
            .is_some_and(|client| client.as_str() != Some(settings.client_id.as_str()))
    {
        return Err(Rejected);
    }
    if let Some(expected) = id.at_hash.as_deref() {
        let hash = digest(&SHA256, pair.access_token.as_bytes());
        if URL_SAFE_NO_PAD.encode(&hash.as_ref()[..16]) != expected {
            return Err(Rejected);
        }
    }
    let permissions = api
        .additional
        .get(&settings.permissions_claim)
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty() && items.len() <= 128)
        .ok_or(Rejected)?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|text| text.len() <= 255)
                .map(str::to_owned)
                .ok_or(Rejected)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Verified {
        subject: id.sub,
        permissions,
        expires_at: id.exp.min(api.exp),
    })
}
