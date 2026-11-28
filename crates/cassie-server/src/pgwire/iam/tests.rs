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
    let identity = token.verify(account, keys, NOW).ok()?;
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
fn should_require_a_valid_signature_from_a_unique_issuer_key() {
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
fn should_verify_an_aws_rsa_key_without_an_optional_algorithm_annotation() {
    // Arrange
    use aws_lc_rs::signature::KeyPair as _;

    let pair =
        aws_lc_rs::signature::RsaKeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048).unwrap();
    let public = pair.public_key();
    let jwks = keys(&json!({
        "kid": "fixture", "kty": "RSA", "use": "sig",
        "n": URL_SAFE_NO_PAD.encode(public.modulus().big_endian_without_leading_zero()),
        "e": URL_SAFE_NO_PAD.encode(public.exponent().big_endian_without_leading_zero()),
    }));
    let trust = TrustSet::from_json(
        &serde_json::to_vec(&json!({
            "audience": AUDIENCE,
            "accounts": [{"account_id": ACCOUNT, "issuer": ISSUER, "algorithms": ["RS256"]}]
        }))
        .unwrap(),
    )
    .unwrap();
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"fixture"}"#);
    let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims()).unwrap());
    let signed = format!("{header}.{body}");
    let mut signature = vec![0; public.modulus_len()];
    pair.sign(
        &aws_lc_rs::signature::RSA_PKCS1_SHA256,
        &aws_lc_rs::rand::SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .unwrap();
    let token = format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature));

    // Act
    let identity = verify(&trust, &jwks, &token);

    // Assert
    assert_eq!(identity, Some((ISSUER.into(), ROLE.into())));
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

fn tls_identities() -> (
    std::sync::Arc<rustls::ServerConfig>,
    tokio_rustls::TlsConnector,
) {
    use std::sync::Arc;
    use tokio_rustls::rustls::{ClientConfig, RootCertStore};

    let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let private = rustls::pki_types::PrivateKeyDer::Pkcs8(
        rustls::pki_types::PrivatePkcs8KeyDer::from(identity.signing_key.serialize_der()),
    );
    let server = Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![identity.cert.der().clone()], private)
            .unwrap(),
    );
    let mut roots = RootCertStore::empty();
    roots.add(identity.cert.der().clone()).unwrap();
    let client = tokio_rustls::TlsConnector::from(Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ));
    (server, client)
}

fn driver_config_for(user: &str) -> tokio_postgres::Config {
    let mut client = tokio_postgres::Config::new();
    client
        .user(user)
        .dbname("postgres")
        .ssl_mode(tokio_postgres::config::SslMode::Require);
    client
}

fn driver_config() -> tokio_postgres::Config {
    driver_config_for(crate::config::AWS_IAM_PGWIRE_SELECTOR)
}

async fn connect_driver(
    address: std::net::SocketAddr,
    connector: &tokio_rustls::TlsConnector,
    credential: &str,
) -> Result<
    (
        tokio_postgres::Client,
        tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
    ),
    tokio_postgres::Error,
> {
    connect_driver_for(
        address,
        connector,
        credential,
        crate::config::AWS_IAM_PGWIRE_SELECTOR,
    )
    .await
}

async fn connect_driver_for(
    address: std::net::SocketAddr,
    connector: &tokio_rustls::TlsConnector,
    credential: &str,
    user: &str,
) -> Result<
    (
        tokio_postgres::Client,
        tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
    ),
    tokio_postgres::Error,
> {
    let mut client = driver_config_for(user);
    client
        .host("localhost")
        .hostaddr(address.ip())
        .port(address.port())
        .password(credential);
    let (client, connection) = client
        .connect(driver_tls::DriverTls(connector.clone()))
        .await?;
    Ok((client, tokio::spawn(connection)))
}

async fn read_role(client: &tokio_postgres::Client) -> String {
    client
        .query_one("SELECT current_user::text AS role_name", &[])
        .await
        .unwrap()
        .get("role_name")
}

async fn start_fixture_pgwire(
    cassie: std::sync::Arc<crate::Cassie>,
    config: crate::config::CassieRuntimeConfig,
    tls: std::sync::Arc<rustls::ServerConfig>,
    issuer: &rotation_tests::IssuerFixture,
    connection_count: usize,
) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    use std::sync::Arc;
    let verifier = Arc::clone(&issuer.verifier);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..connection_count {
            let (socket, peer) = listener.accept().await.unwrap();
            Box::pin(crate::pgwire::connection::run_connection_with_iam(
                socket,
                Arc::clone(&cassie),
                config.clone(),
                Some(Arc::clone(&tls)),
                false,
                peer.ip(),
                Some(Arc::clone(&verifier)),
            ))
            .await;
        }
    });
    (address, server)
}

fn install_driver_grants(cassie: &crate::Cassie, admin: &crate::app::CassieSession) {
    for sql in [
        "CREATE ROLE service EXTERNAL LOGIN",
        &format!("ALTER ROLE service BIND AWS IAM ROLE '{ROLE}' ISSUER '{ISSUER}'"),
        "CREATE TABLE driver_visible (value INT)",
        "GRANT USAGE ON SCHEMA public TO service",
        "GRANT SELECT ON TABLE driver_visible TO service",
    ] {
        cassie.execute_sql(admin, sql, vec![]).unwrap();
    }
}

async fn signed_driver_connections() {
    use std::sync::Arc;

    // Arrange
    let config = crate::config::CassieRuntimeConfig {
        pgwire_aws_iam_trust_file: Some("fixture-trust.json".into()),
        password: "test-root".into(),
        ..crate::config::CassieRuntimeConfig::default()
    };
    let cassie = Arc::new(crate::Cassie::new_in_memory_with_config(config.clone()).unwrap());
    cassie.startup().unwrap();
    let admin = cassie.create_session("root", None);
    install_driver_grants(&cassie, &admin);
    let (server_tls, client_tls) = tls_identities();
    let signing_key = signing_key();
    let issuer =
        rotation_tests::IssuerFixture::start(vec![rotation_tests::response(&[jwk(&signing_key)])])
            .await;
    let (address, server) =
        start_fixture_pgwire(Arc::clone(&cassie), config, server_tls, &issuer, 4).await;

    // Act
    let plaintext = driver_config()
        .ssl_mode(tokio_postgres::config::SslMode::Disable)
        .connect_raw(
            tokio::net::TcpStream::connect(address).await.unwrap(),
            tokio_postgres::NoTls,
        )
        .await
        .err()
        .expect("the plaintext selector was rejected");
    let original_token = sign(&signing_key, "fixture", &claims());
    let (original, original_connection) = connect_driver(address, &client_tls, &original_token)
        .await
        .unwrap();
    let original_role = read_role(&original).await;
    let permitted = original
        .simple_query("SELECT value FROM driver_visible")
        .await;
    cassie
        .execute_sql(
            &admin,
            "REVOKE SELECT ON TABLE driver_visible FROM service",
            vec![],
        )
        .unwrap();
    let denied = original
        .simple_query("SELECT value FROM driver_visible")
        .await;
    drop(original);
    original_connection.await.unwrap().unwrap();
    let later = NOW + 200;
    issuer.clock.set(later);
    let expired = connect_driver(address, &client_tls, &original_token)
        .await
        .expect_err("expired token denied");
    let mut refreshed_claims = claims();
    refreshed_claims["iat"] = json!(later);
    refreshed_claims["exp"] = json!(later + 200);
    let refreshed_token = sign(&signing_key, "fixture", &refreshed_claims);
    let (refreshed, refreshed_connection) = connect_driver(address, &client_tls, &refreshed_token)
        .await
        .unwrap();
    let refreshed_role = read_role(&refreshed).await;
    drop(refreshed);
    refreshed_connection.await.unwrap().unwrap();

    // Assert
    let auth_failed = Some(&tokio_postgres::error::SqlState::INVALID_AUTHORIZATION_SPECIFICATION);
    assert_eq!(
        plaintext.code(),
        auth_failed,
        "plaintext rejected before password prompt"
    );
    assert_eq!(original_role, "service");
    assert!(permitted.is_ok());
    assert_eq!(
        denied.unwrap_err().code(),
        Some(&tokio_postgres::error::SqlState::INSUFFICIENT_PRIVILEGE)
    );
    assert_eq!(expired.code(), auth_failed);
    assert_eq!(refreshed_role, "service");
    assert_eq!(issuer.requests(), 1);
    server.await.unwrap();
}

#[test]
fn should_require_current_iam_authority_for_driver_connections() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let result = runtime.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            signed_driver_connections(),
        )
        .await
    });

    // Assert
    result.expect("fixture completed within its hang guard");
}

#[test]
fn should_never_challenge_for_credentials_on_invalid_iam_transport_configuration() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let config = crate::config::CassieRuntimeConfig {
            pgwire_aws_iam_trust_file: Some("fixture-trust.json".into()),
            password: "test-root".into(),
            ..crate::config::CassieRuntimeConfig::default()
        };
        let cassie = Arc::new(crate::Cassie::new_in_memory_with_config(config.clone()).unwrap());
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, peer) = listener.accept().await.unwrap();
            Box::pin(crate::pgwire::connection::run_connection(
                socket,
                cassie,
                config,
                None,
                false,
                peer.ip(),
            ))
            .await;
        });
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();

        // Act
        let mut startup = vec![0, 0, 0, 0, 0, 3, 0, 0];
        startup.extend_from_slice(b"user\0cassie_aws_iam\0database\0postgres\0\0");
        let packet_len = u32::try_from(startup.len()).unwrap().to_be_bytes();
        startup[..4].copy_from_slice(&packet_len);
        let _ = socket.write_all(&startup).await;
        let mut bytes = [0; 1];
        let read = tokio::time::timeout(std::time::Duration::from_secs(5), socket.read(&mut bytes))
            .await
            .expect("transport did not hang");

        // Assert
        assert!(
            !matches!(read, Ok(1)),
            "unexpected auth message tag {}",
            bytes[0]
        );
        server.await.unwrap();
    });
}

#[path = "rotation_tests.rs"]
mod rotation_tests;

#[path = "workload_tests.rs"]
mod workload_tests;

#[path = "portal_tests.rs"]
mod portal_tests;

#[path = "driver_tls.rs"]
mod driver_tls;
