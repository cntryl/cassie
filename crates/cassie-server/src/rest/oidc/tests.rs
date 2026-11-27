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

mod issuer {
    use std::collections::HashMap;
    use std::convert::Infallible;
    use std::sync::{Arc, OnceLock};

    use http_body_util::{BodyExt, Full};
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{body::Incoming, Request, Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use ring::digest::{digest, SHA256};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    use super::{json, jwks, sign, Engine as _, URL_SAFE_NO_PAD};
    use crate::app::Cassie;
    use crate::config::CassieRuntimeConfig;
    use crate::rest::oidc::{now, provider, Oidc, Settings};

    struct BrowserRequest {
        nonce: String,
        challenge: String,
    }

    struct Issuer {
        base: String,
        public_keys: Vec<u8>,
        signing: aws_lc_rs::signature::RsaKeyPair,
        browser: OnceLock<BrowserRequest>,
    }

    impl Issuer {
        async fn reply(&self, request: Request<Incoming>) -> Response<Full<bytes::Bytes>> {
            match request.uri().path() {
                "/.well-known/openid-configuration" => Response::new(Full::new(
                    json!({
                        "issuer": self.base,
                        "authorization_endpoint": format!("{}authorize", self.base),
                        "token_endpoint": format!("{}token", self.base),
                        "jwks_uri": format!("{}jwks", self.base),
                        "token_endpoint_auth_methods_supported": ["client_secret_post"]
                    })
                    .to_string()
                    .into(),
                )),
                "/jwks" => Response::new(Full::new(self.public_keys.clone().into())),
                "/token" => self.exchange(request).await,
                _ => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Full::default())
                    .expect("missing endpoint response"),
            }
        }

        async fn exchange(&self, request: Request<Incoming>) -> Response<Full<bytes::Bytes>> {
            let body = request
                .into_body()
                .collect()
                .await
                .expect("read grant request")
                .to_bytes();
            let form = url::form_urlencoded::parse(&body).collect::<HashMap<_, _>>();
            let value = |name| form.get(name).map_or("", std::borrow::Cow::as_ref);
            let proof = self.browser.get().expect("browser reached authorization");
            let challenge =
                URL_SAFE_NO_PAD.encode(digest(&SHA256, value("code_verifier").as_bytes()).as_ref());
            assert_eq!(value("client_secret"), "fixture-secret");
            assert_eq!(value("code"), "fixture-code");
            assert_eq!(value("grant_type"), "authorization_code");
            assert_eq!(challenge, proof.challenge);
            assert_eq!(
                value("redirect_uri"),
                "https://ui.example/api/v1/auth/oidc/callback"
            );
            let issued = now().expect("unix time");
            let api = sign(
                &self.signing,
                &json!({
                    "iss": self.base, "aud": "cassie-api", "sub": "browser", "iat": issued,
                    "exp": issued + 300, "permissions": ["db:analytics"]
                }),
            );
            let hash = digest(&SHA256, api.as_bytes());
            let identity = sign(
                &self.signing,
                &json!({
                    "iss": self.base, "aud": "cassie-client", "sub": "browser", "iat": issued,
                    "exp": issued + 600, "nonce": proof.nonce,
                    "at_hash": URL_SAFE_NO_PAD.encode(&hash.as_ref()[..16])
                }),
            );
            Response::new(Full::new(
                json!({"id_token": identity, "access_token": api})
                    .to_string()
                    .into(),
            ))
        }
    }

    async fn start() -> (Oidc, Arc<Issuer>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind issuer");
        let address = listener.local_addr().expect("issuer address");
        let base = format!("https://localhost:{}/", address.port());
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
            .expect("test certificate");
        let builder = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("TLS versions");
        let private_key =
            rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.signing_key.serialize_der());
        let tls = builder
            .with_no_client_auth()
            .with_single_cert(vec![certificate.cert.der().clone()], private_key.into())
            .expect("TLS identity");
        let root = reqwest::Certificate::from_der(certificate.cert.der()).expect("test trust root");
        let client = provider::client_builder()
            .tls_certs_merge([root])
            .no_proxy()
            .resolve("localhost", address)
            .build()
            .expect("configured HTTP client");
        let signing = aws_lc_rs::signature::RsaKeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048)
            .expect("issuer signing key");
        let issuer = Arc::new(Issuer {
            base: base.clone(),
            public_keys: jwks(&signing),
            signing,
            browser: OnceLock::new(),
        });
        let settings = Settings::new(
            base,
            "cassie-client".into(),
            "fixture-secret".into(),
            "cassie-api".into(),
            "https://ui.example/api/v1/auth/oidc/callback".into(),
            "external_reader".into(),
            "permissions".into(),
        )
        .expect("issuer settings");
        let owner = Arc::clone(&issuer);
        let task = tokio::spawn(async move {
            let acceptor = TlsAcceptor::from(Arc::new(tls));
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let owner = Arc::clone(&owner);
                tokio::spawn(async move {
                    let stream = acceptor.accept(stream).await.expect("accept issuer TLS");
                    let service = service_fn(move |request| {
                        let owner = Arc::clone(&owner);
                        async move { Ok::<_, Infallible>(owner.reply(request).await) }
                    });
                    let _ = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        (Oidc::new(settings, client), issuer, task)
    }

    #[test]
    fn should_finish_browser_login_against_the_bound_https_issuer() {
        // Arrange
        let config = CassieRuntimeConfig {
            password: "oidc-test-root-password".to_owned(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_in_memory_with_config(config).expect("in-memory Cassie");
        cassie.startup().expect("startup");
        cassie
            .create_role("external_reader", false, None, false)
            .expect("map role");
        cassie
            .create_logical_database("analytics", false)
            .expect("create granted database");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        // Act
        let (completed, enabled, second_callback, after_logout) = runtime.block_on(async {
            let (oidc, issuer, listener) = start().await;
            let started = oidc.start("/query").await.expect("start browser login");
            let params = started.location.query_pairs().collect::<HashMap<_, _>>();
            let param = |name| {
                params
                    .get(name)
                    .expect("authorization parameter")
                    .to_string()
            };
            assert_eq!(param("code_challenge_method"), "S256");
            assert_eq!(param("audience"), "cassie-api");
            assert!(issuer
                .browser
                .set(BrowserRequest {
                    nonce: param("nonce"),
                    challenge: param("code_challenge"),
                })
                .is_ok());
            let state = param("state");
            let completed = oidc
                .complete(&cassie, &state, &started.browser, "fixture-code")
                .await
                .expect("verified callback");
            let session = oidc.session(&completed.token).expect("opaque session");
            let principal = cassie
                .external_principal(&session.authority)
                .expect("revalidate role");
            let enabled = (
                principal.role.can_access_database("analytics"),
                principal.role.can_access_database("postgres"),
            );
            let second_callback = oidc
                .complete(&cassie, &state, &started.browser, "fixture-code")
                .await;
            oidc.revoke(&completed.token);
            let after_logout = oidc.session(&completed.token);
            listener.abort();
            (completed, enabled, second_callback, after_logout)
        });

        // Assert
        assert_eq!(completed.location, "/query");
        assert!((1..=300).contains(&completed.max_age));
        assert_eq!(enabled, (true, false));
        assert!(second_callback.is_err());
        assert!(after_logout.is_none());
    }
}
