use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ring::signature;
use serde::Deserialize;
use std::collections::HashMap;

use super::{Algorithm, Rejected, MAX_JWKS_BYTES};

enum VerificationKey {
    Rsa { modulus: Vec<u8>, exponent: Vec<u8> },
    P384([u8; 97]),
}

impl VerificationKey {
    fn algorithm(&self) -> Algorithm {
        match self {
            Self::Rsa { .. } => Algorithm::Rs256,
            Self::P384(_) => Algorithm::Es384,
        }
    }

    fn verify(&self, message: &[u8], signature: &[u8]) -> Result<(), Rejected> {
        match self {
            Self::Rsa { modulus, exponent } => signature::RsaPublicKeyComponents {
                n: modulus,
                e: exponent,
            }
            .verify(&signature::RSA_PKCS1_2048_8192_SHA256, message, signature),
            Self::P384(point) => {
                signature::UnparsedPublicKey::new(&signature::ECDSA_P384_SHA384_FIXED, point)
                    .verify(message, signature)
            }
        }
        .map_err(|_| Rejected)
    }
}

#[derive(Deserialize)]
struct JwksDocument {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    alg: Option<Algorithm>,
    #[serde(rename = "use")]
    key_use: Option<String>,
    key_ops: Option<Vec<String>>,
    crv: Option<String>,
    x: Option<String>,
    y: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

fn decode(value: Option<&str>) -> Result<Vec<u8>, Rejected> {
    URL_SAFE_NO_PAD
        .decode(value.ok_or(Rejected)?)
        .map_err(|_| Rejected)
}

impl Jwk {
    fn verification_key(&self) -> Result<VerificationKey, Rejected> {
        if self.key_use.as_deref().is_some_and(|value| value != "sig")
            || self
                .key_ops
                .as_ref()
                .is_some_and(|ops| !ops.iter().any(|op| op == "verify"))
        {
            return Err(Rejected);
        }
        let key = match (self.kty.as_str(), self.crv.as_deref()) {
            ("RSA", None) => {
                let modulus = decode(self.n.as_deref())?;
                let exponent = decode(self.e.as_deref())?;
                if !(256..=1_024).contains(&modulus.len()) || !(1..=5).contains(&exponent.len()) {
                    return Err(Rejected);
                }
                VerificationKey::Rsa { modulus, exponent }
            }
            ("EC", Some("P-384")) => {
                let x = decode(self.x.as_deref())?;
                let y = decode(self.y.as_deref())?;
                if x.len() != 48 || y.len() != 48 {
                    return Err(Rejected);
                }
                let mut point = [0; 97];
                point[0] = 4;
                point[1..49].copy_from_slice(&x);
                point[49..].copy_from_slice(&y);
                VerificationKey::P384(point)
            }
            _ => return Err(Rejected),
        };
        if self.alg.is_some_and(|alg| alg != key.algorithm()) {
            return Err(Rejected);
        }
        Ok(key)
    }
}

pub(super) struct IssuerKeys(HashMap<String, VerificationKey>);

pub(super) fn valid_key_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_graphic())
}

impl IssuerKeys {
    pub(super) fn from_json(bytes: &[u8]) -> Result<Self, Rejected> {
        if bytes.len() > MAX_JWKS_BYTES {
            return Err(Rejected);
        }
        let doc: JwksDocument = serde_json::from_slice(bytes).map_err(|_| Rejected)?;
        if doc.keys.is_empty() || doc.keys.len() > 32 {
            return Err(Rejected);
        }
        let mut keys = HashMap::new();
        for jwk in doc.keys {
            if !valid_key_id(&jwk.kid) {
                return Err(Rejected);
            }
            let key = jwk.verification_key()?;
            if keys.insert(jwk.kid, key).is_some() {
                return Err(Rejected);
            }
        }
        Ok(Self(keys))
    }

    pub(super) fn has_key(&self, kid: &str, alg: Algorithm) -> bool {
        self.0.get(kid).is_some_and(|key| key.algorithm() == alg)
    }

    pub(super) fn verify(
        &self,
        kid: &str,
        alg: Algorithm,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), Rejected> {
        self.0
            .get(kid)
            .filter(|key| key.algorithm() == alg)
            .ok_or(Rejected)?
            .verify(message, signature)
    }
}
