use aws_lc_rs::signature::KeyPair as _;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};

use super::tokens::{verify, TokenPair};
use super::{safe_return, Settings, Store};

fn settings() -> Settings {
    Settings::new(
        "https://issuer.example/".into(),
        "console-client".into(),
        "fake-secret".into(),
        "https://cassie.example/api".into(),
        "https://cassie.example/api/v1/auth/oidc/callback".into(),
        "external_reader".into(),
        "permissions".into(),
    )
    .ok()
    .expect("settings")
}

fn sign(key: &aws_lc_rs::signature::RsaKeyPair, claims: &Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"fixture"}"#);
    let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("encode claims"));
    let signing_input = format!("{header}.{claims}");
    let mut signature = vec![0; key.public_key().modulus_len()];
    key.sign(
        &aws_lc_rs::signature::RSA_PKCS1_SHA256,
        &aws_lc_rs::rand::SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .expect("sign claims");
    format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(signature))
}

fn jwks(key: &aws_lc_rs::signature::RsaKeyPair) -> Vec<u8> {
    let public = key.public_key();
    serde_json::to_vec(&json!({"keys": [{
        "kid": "fixture", "alg": "RS256", "kty": "RSA", "use": "sig",
        "n": URL_SAFE_NO_PAD.encode(public.modulus().big_endian_without_leading_zero()),
        "e": URL_SAFE_NO_PAD.encode(public.exponent().big_endian_without_leading_zero()),
    }]}))
    .expect("serialize public keys")
}

#[test]
fn should_use_only_the_api_token_permissions_given_distinct_verified_audiences() {
    // Arrange
    let key = aws_lc_rs::signature::RsaKeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048)
        .expect("RSA key");
    let id = json!({
        "iss": "https://issuer.example/", "sub": "fixture", "aud": "console-client",
        "iat": 900, "exp": 1_700, "nonce": "nonce", "permissions": ["db:*"]
    });
    let api = json!({
        "iss": "https://issuer.example/", "sub": "fixture",
        "aud": ["https://cassie.example/api", "https://issuer.example/userinfo"],
        "iat": 900, "exp": 1_600, "azp": "console-client", "permissions": ["db:analytics"]
    });
    let pair = TokenPair {
        id_token: sign(&key, &id),
        access_token: sign(&key, &api),
    };

    // Act
    let verified = verify(&settings(), &jwks(&key), &pair, "nonce", 1_000).expect("verified login");

    // Assert
    assert_eq!(verified.permissions, ["db:analytics"]);
    assert_eq!(verified.expires_at, 1_600);
}

#[test]
fn should_reject_token_pairs_that_disagree_about_the_subject() {
    // Arrange
    let key = aws_lc_rs::signature::RsaKeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048)
        .expect("RSA key");
    let id = json!({
        "iss": "https://issuer.example/", "sub": "user-one", "aud": "console-client",
        "iat": 900, "exp": 1_700, "nonce": "nonce"
    });
    let api = json!({
        "iss": "https://issuer.example/", "sub": "user-two", "aud": "https://cassie.example/api",
        "iat": 900, "exp": 1_600, "permissions": ["db:*"]
    });
    let pair = TokenPair {
        id_token: sign(&key, &id),
        access_token: sign(&key, &api),
    };

    // Act
    let verified = verify(&settings(), &jwks(&key), &pair, "nonce", 1_000);

    // Assert
    assert!(verified.is_err());
}

#[test]
fn should_consume_state_once_for_only_the_bound_browser() {
    // Arrange
    let mut store = Store::default();
    let (state, browser, _, _) = store.begin("/analytics", 1_000).expect("challenge");

    // Act
    let unrelated = store.take(&state, "wrong-browser", 1_001);
    let bound = store.take(&state, &browser, 1_001);
    let replay = store.take(&state, &browser, 1_001);

    // Assert
    assert!(unrelated.is_err());
    assert_eq!(bound.expect("one callback").next, "/analytics");
    assert!(replay.is_err());
}

#[test]
fn should_replace_off_origin_return_locations_with_the_local_default() {
    // Arrange
    let paths = [
        "https://elsewhere.example",
        "//elsewhere.example",
        "/\\elsewhere.example",
        "/query?a=1",
    ];

    // Act
    let destinations = paths.map(safe_return);

    // Assert
    assert_eq!(destinations, ["/", "/", "/", "/query?a=1"]);
}
