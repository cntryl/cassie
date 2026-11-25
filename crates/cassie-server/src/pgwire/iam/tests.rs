use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P384_SHA384_FIXED_SIGNING};
use serde_json::{json, Value};

use super::{IssuerKeys, Token, TrustSet};

const ISSUER: &str = "https://example-account.tokens.sts.global.api.aws";
const AUDIENCE: &str = "https://cassie.example/sandbox";
const ACCOUNT: &str = "123456789012";
const ROLE: &str = "arn:aws:iam::123456789012:role/services/projector";
const NOW: u64 = 1_800_000_000;

fn trust() -> TrustSet {
    TrustSet::from_json(
        &serde_json::to_vec(&json!({
            "audience": AUDIENCE,
            "accounts": [{"account_id": ACCOUNT, "issuer": ISSUER, "algorithms": ["ES384"]}]
        }))
        .unwrap(),
    )
    .expect("test trust")
}

fn signing_key() -> EcdsaKeyPair {
    let random = SystemRandom::new();
    let seed = EcdsaKeyPair::generate_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, &random).unwrap();
    EcdsaKeyPair::from_pkcs8(&ECDSA_P384_SHA384_FIXED_SIGNING, seed.as_ref(), &random).unwrap()
}

fn jwk(key: &EcdsaKeyPair) -> Value {
    let point = key.public_key().as_ref();
    assert_eq!(point[0], 4);
    json!({
        "kty": "EC", "use": "sig", "kid": "fixture", "alg": "ES384", "crv": "P-384",
        "x": URL_SAFE_NO_PAD.encode(&point[1..49]),
        "y": URL_SAFE_NO_PAD.encode(&point[49..]),
    })
}

fn keys(jwk: &Value) -> IssuerKeys {
    IssuerKeys::from_json(&serde_json::to_vec(&json!({"keys": [jwk]})).unwrap())
        .expect("fixture key set")
}

fn claims() -> Value {
    json!({
        "iss": ISSUER, "aud": AUDIENCE, "sub": ROLE, "iat": NOW - 100, "exp": NOW + 200,
        "https://sts.amazonaws.com/": {"aws_account": ACCOUNT, "principal_id": ROLE}
    })
}

fn sign(key: &EcdsaKeyPair, kid: &str, claims: &Value) -> String {
    let header =
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"alg": "ES384", "kid": kid})).unwrap());
    let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
    let signed = format!("{header}.{claims}");
    let signature = key.sign(&SystemRandom::new(), signed.as_bytes()).unwrap();
    format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature.as_ref()))
}

fn verify(trust: &TrustSet, keys: &IssuerKeys, token: &str) -> Option<(String, String)> {
    let token = Token::parse(token).ok()?;
    let account = trust.account_for(&token).ok()?;
    let identity = token.verify(trust, account, keys, NOW).ok()?;
    Some((identity.issuer, identity.role_arn))
}

#[test]
fn should_verify_a_signed_exact_aws_identity_with_the_configured_audience() {
    // Arrange
    let key = signing_key();
    let keys = keys(&jwk(&key));
    let mut claims = claims();
    claims["aud"] = json!([AUDIENCE]);
    let signed = sign(&key, "fixture", &claims);

    // Act
    let principal = verify(&trust(), &keys, &signed);

    // Assert
    assert_eq!(principal, Some((ISSUER.into(), ROLE.into())));
}

#[test]
fn should_reject_signed_tokens_with_wrong_issuer_audience_time_or_role_claims() {
    // Arrange
    let key = signing_key();
    let keys = keys(&jwk(&key));
    let trust = trust();
    let changes = [
        (
            "/iss",
            json!("https://other-account.tokens.sts.global.api.aws"),
        ),
        ("/aud", json!("https://another.example")),
        ("/aud", json!([AUDIENCE, "https://another.example"])),
        ("/exp", json!(NOW)),
        ("/exp", json!(NOW + 201)),
        ("/iat", json!(NOW + 31)),
        ("/sub", json!("arn:aws:iam::123456789012:user/root")),
        (
            "/https:~1~1sts.amazonaws.com~1/aws_account",
            json!("000000000000"),
        ),
        (
            "/https:~1~1sts.amazonaws.com~1/principal_id",
            json!("arn:aws:iam::123456789012:role/other"),
        ),
    ];

    // Act
    let outcomes = changes.map(|(path, value)| {
        let mut claims = claims();
        *claims.pointer_mut(path).unwrap() = value;
        verify(&trust, &keys, &sign(&key, "fixture", &claims))
    });

    // Assert
    assert!(outcomes.into_iter().all(|outcome| outcome.is_none()));
}

#[test]
fn should_reject_a_forged_signature_unknown_key_and_duplicate_key_ids() {
    // Arrange
    let key = signing_key();
    let forged = signing_key();
    let jwk = jwk(&key);
    let keys = keys(&jwk);
    let trust = trust();

    // Act
    let forged = verify(&trust, &keys, &sign(&forged, "fixture", &claims()));
    let unknown = verify(&trust, &keys, &sign(&key, "different-key", &claims()));
    let duplicate =
        IssuerKeys::from_json(&serde_json::to_vec(&json!({"keys": [jwk.clone(), jwk]})).unwrap());

    // Assert
    assert!(forged.is_none());
    assert!(unknown.is_none());
    assert!(duplicate.is_err());
}

#[test]
fn should_only_derive_key_endpoints_from_exact_https_aws_account_issuers() {
    // Arrange
    let hosts = [
        ISSUER,
        "http://example-account.tokens.sts.global.api.aws",
        "https://example-account.tokens.sts.global.api.aws.example.com",
        "https://example-account.tokens.sts.global.api.aws:444",
        "https://example-account.tokens.sts.global.api.aws/other",
        "https://operator@example-account.tokens.sts.global.api.aws",
    ];

    // Act
    let accepted = hosts.map(|issuer| {
        TrustSet::from_json(
            &serde_json::to_vec(&json!({
                "audience": AUDIENCE,
                "accounts": [{"account_id": ACCOUNT, "issuer": issuer, "algorithms": ["ES384"]}]
            }))
            .unwrap(),
        )
        .is_ok()
    });

    // Assert
    assert_eq!(accepted, [true, false, false, false, false, false]);
}
