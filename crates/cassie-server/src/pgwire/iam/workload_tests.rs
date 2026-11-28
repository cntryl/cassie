use std::sync::Arc;

use ring::signature::{RsaKeyPair, RSA_PKCS1_SHA256};

use super::{
    connect_driver_for, keys, read_role, rotation_tests, start_fixture_pgwire, tls_identities,
    verify, TrustSet, NOW, URL_SAFE_NO_PAD,
};
use base64::Engine as _;
use serde_json::{json, Value};

const AZURE_TENANT: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
const AZURE_ISSUER: &str =
    "https://login.microsoftonline.com/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/v2.0";
const AZURE_V1_ISSUER: &str = "https://sts.windows.net/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/";
const AZURE_ACTOR: &str = "eeeeeeee-bbbb-cccc-dddd-aaaaaaaaaaaa";
const AZURE_SUBJECT: &str = "ffffffff-bbbb-cccc-dddd-aaaaaaaaaaaa";
const AZURE_AUDIENCE: &str = "00000000-1111-2222-3333-444444444444";
const AZURE_V1_AUDIENCE: &str = "api://00000000-1111-2222-3333-444444444444";
const GOOGLE_ISSUER: &str = "https://accounts.google.com";
const GOOGLE_SUBJECT: &str = "112010400000000710080";
const GOOGLE_AUDIENCE: &str = "https://cassie.example/google";

fn workload_trust() -> TrustSet {
    TrustSet::from_json(
        &serde_json::to_vec(&json!({
            "azure": [
                {"tenant_id": AZURE_TENANT, "token_version": "2.0", "audience": AZURE_AUDIENCE},
                {"tenant_id": AZURE_TENANT, "token_version": "1.0", "audience": AZURE_V1_AUDIENCE}
            ],
            "google": {"audience": GOOGLE_AUDIENCE}
        }))
        .unwrap(),
    )
    .unwrap()
}

fn rsa_jwk() -> Value {
    serde_json::from_str(include_str!("testdata/test-rsa.jwk.json")).unwrap()
}

fn sign(claims: &Value) -> String {
    let key = RsaKeyPair::from_pkcs8(include_bytes!("testdata/insecure-test-rsa.der")).unwrap();
    let header = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(
            &json!({"alg":"RS256", "kid":"workload-fixture", "typ":"JWT", "x5t":"fixture"}),
        )
        .unwrap(),
    );
    let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
    let message = format!("{header}.{body}");
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        message.as_bytes(),
        &mut signature,
    )
    .unwrap();
    format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature))
}

fn azure_claims() -> Value {
    json!({
        "iss": AZURE_ISSUER, "aud": AZURE_AUDIENCE,
        "sub": "application-specific-subject", "oid": AZURE_SUBJECT,
        "tid": AZURE_TENANT, "ver": "2.0", "idtyp": "app", "azp": AZURE_ACTOR,
        "iat": NOW - 10, "exp": NOW + 80_000
    })
}

fn google_claims() -> Value {
    json!({
        "iss": GOOGLE_ISSUER, "aud": GOOGLE_AUDIENCE,
        "sub": GOOGLE_SUBJECT, "azp": GOOGLE_SUBJECT,
        "iat": NOW - 10, "exp": NOW + 3500
    })
}

#[test]
fn should_resolve_the_azure_object_id_for_an_entra_v1_app_token() {
    // Arrange
    let trust = workload_trust();
    let keys = keys(&rsa_jwk());
    let mut claims = azure_claims();
    claims["iss"] = json!(AZURE_V1_ISSUER);
    claims["aud"] = json!(AZURE_V1_AUDIENCE);
    claims["ver"] = json!("1.0");
    claims["appid"] = json!(AZURE_ACTOR);
    claims.as_object_mut().unwrap().remove("azp");

    // Act
    let principal = verify(&trust, &keys, &sign(&claims));

    // Assert
    assert_eq!(
        principal,
        Some((AZURE_V1_ISSUER.into(), AZURE_SUBJECT.into()))
    );
}

#[test]
fn should_reject_azure_tokens_that_do_not_prove_the_configured_app_identity() {
    // Arrange
    let trust = workload_trust();
    let keys = keys(&rsa_jwk());
    let changes = [
        ("iss", json!(AZURE_V1_ISSUER)),
        ("aud", json!(GOOGLE_AUDIENCE)),
        ("tid", json!(AZURE_ACTOR)),
        ("idtyp", json!("user")),
        ("oid", json!("invalid")),
        ("azp", json!("invalid")),
        ("ver", json!("1.0")),
        ("scp", json!("delegated")),
    ];

    // Act
    let results = changes.map(|(name, value)| {
        let mut claims = azure_claims();
        claims[name] = value;
        verify(&trust, &keys, &sign(&claims))
    });

    // Assert
    assert!(results.into_iter().all(|principal| principal.is_none()));
}

#[test]
fn should_reject_a_google_token_with_an_unrelated_authorized_party() {
    // Arrange
    let trust = workload_trust();
    let keys = keys(&rsa_jwk());
    let mut claims = google_claims();
    claims["azp"] = json!("8876543210.apps.googleusercontent.com");

    // Act
    let principal = verify(&trust, &keys, &sign(&claims));

    // Assert
    assert!(principal.is_none());
}

async fn read_cloud_role(issuer: &str, subject: &str, claims: &Value) -> String {
    let config = crate::config::CassieRuntimeConfig {
        pgwire_workload_trust_file: Some("fixture-workload.json".into()),
        password: "test-root".into(),
        ..crate::config::CassieRuntimeConfig::default()
    };
    let cassie = Arc::new(crate::Cassie::new_in_memory_with_config(config.clone()).unwrap());
    cassie.startup().unwrap();
    let admin = cassie.create_session("root", None);
    for sql in [
        "CREATE ROLE workload EXTERNAL LOGIN",
        &format!("ALTER ROLE workload BIND WORKLOAD IDENTITY '{subject}' ISSUER '{issuer}'"),
    ] {
        cassie.execute_sql(&admin, sql, vec![]).unwrap();
    }
    let (server_tls, client_tls) = tls_identities();
    let jwks = rotation_tests::IssuerFixture::start_for(
        workload_trust(),
        issuer,
        vec![rotation_tests::response(&[rsa_jwk()])],
    )
    .await;
    let (address, server) = start_fixture_pgwire(cassie, config, server_tls, &jwks, 1).await;
    let (client, connection) = connect_driver_for(
        address,
        &client_tls,
        &sign(claims),
        crate::config::WORKLOAD_PGWIRE_SELECTOR,
    )
    .await
    .unwrap();
    let role = read_role(&client).await;
    drop(client);
    connection.await.unwrap().unwrap();
    server.await.unwrap();
    role
}

#[test]
fn should_map_azure_managed_identity_to_the_bound_pgwire_role() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Act
    let role = runtime.block_on(read_cloud_role(
        AZURE_ISSUER,
        AZURE_SUBJECT,
        &azure_claims(),
    ));

    // Assert
    assert_eq!(role, "workload");
}

#[test]
fn should_map_google_service_account_to_the_bound_pgwire_role() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // Act
    let role = runtime.block_on(read_cloud_role(
        GOOGLE_ISSUER,
        GOOGLE_SUBJECT,
        &google_claims(),
    ));

    // Assert
    assert_eq!(role, "workload");
}
