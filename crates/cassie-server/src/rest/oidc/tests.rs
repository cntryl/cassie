use aws_lc_rs::signature::KeyPair as _;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde_json::{json, Value};

use super::tokens::{verify, TokenPair};
use super::{authorization_scope, random, safe_return, Settings, Store};

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
    let nonce = random().expect("nonce");
    let id = json!({
        "iss": "https://issuer.example/", "sub": "fixture", "aud": "console-client",
        "iat": 900, "exp": 1_700, "nonce": nonce.as_str(), "permissions": ["db:*"]
    });
    let api = json!({
        "iss": "https://issuer.example/", "sub": "fixture",
        "aud": ["https://cassie.example/api", "https://issuer.example/userinfo"],
        "iat": 900, "exp": 1_600, "azp": "console-client", "client_id": "console-client",
        "permissions": ["db:analytics"]
    });
    let pair = TokenPair {
        id_token: sign(&key, &id),
        access_token: sign(&key, &api),
    };

    // Act
    let verified = verify(&settings(), &jwks(&key), &pair, &nonce, 1_000).expect("verified login");

    // Assert
    assert_eq!(verified.permissions, ["db:analytics"]);
    assert_eq!(verified.expires_at, 1_600);
}

#[test]
fn should_reject_token_pairs_that_disagree_about_the_subject() {
    // Arrange
    let key = aws_lc_rs::signature::RsaKeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048)
        .expect("RSA key");
    let nonce = random().expect("nonce");
    let id = json!({
        "iss": "https://issuer.example/", "sub": "user-one", "aud": "console-client",
        "iat": 900, "exp": 1_700, "nonce": nonce.as_str()
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
    let verified = verify(&settings(), &jwks(&key), &pair, &nonce, 1_000);

    // Assert
    assert!(verified.is_err());
}

#[test]
fn should_reject_a_signed_api_token_given_a_different_requesting_client() {
    // Arrange
    let key = aws_lc_rs::signature::RsaKeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048)
        .expect("RSA key");
    let nonce = random().expect("nonce");
    let id = json!({
        "iss": "https://issuer.example/", "sub": "fixture", "aud": "console-client",
        "iat": 900, "exp": 1_700, "nonce": nonce.as_str()
    });
    let api = json!({
        "iss": "https://issuer.example/", "sub": "fixture", "aud": "https://cassie.example/api",
        "iat": 900, "exp": 1_600, "azp": "console-client", "client_id": "other-client",
        "permissions": ["db:*"]
    });
    let pair = TokenPair {
        id_token: sign(&key, &id),
        access_token: sign(&key, &api),
    };

    // Act
    let verified = verify(&settings(), &jwks(&key), &pair, &nonce, 1_000);

    // Assert
    assert!(verified.is_err());
}

#[test]
fn should_reject_invalid_oauth_scope_settings() {
    // Arrange
    let too_long = "x".repeat(129);
    let too_many = (0..33)
        .map(|index| format!("scope:{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let over_budget = " ".repeat(1025);
    let malformed = [
        "read\twrite",
        "read\nwrite",
        "read\\write",
        "read\"write",
        "rèad",
        &too_long,
        &too_many,
        &over_budget,
    ];

    // Act
    let valid = authorization_scope(" read:db  openid profile read:db ").expect("OAuth scopes");
    let default = authorization_scope("").expect("mandatory scope");
    let rejected = malformed.map(|value| authorization_scope(value).is_err());

    // Assert
    assert_eq!(valid, "openid read:db profile");
    assert_eq!(default, "openid");
    assert_eq!(rejected, [true; 8]);
}

#[test]
fn should_reject_an_access_token_that_disagrees_with_the_signed_access_hash() {
    // Arrange
    let key = aws_lc_rs::signature::RsaKeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048)
        .expect("RSA key");
    let nonce = random().expect("nonce");
    let id = json!({
        "iss": "https://issuer.example/", "sub": "fixture", "aud": "console-client",
        "iat": 900, "exp": 1_700, "nonce": nonce.as_str(), "at_hash": "unmatched"
    });
    let api = json!({
        "iss": "https://issuer.example/", "sub": "fixture", "aud": "https://cassie.example/api",
        "iat": 900, "exp": 1_600, "permissions": ["db:*"]
    });
    let pair = TokenPair {
        id_token: sign(&key, &id),
        access_token: sign(&key, &api),
    };

    // Act
    let verified = verify(&settings(), &jwks(&key), &pair, &nonce, 1_000);

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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, OnceLock};

    use http_body_util::{BodyExt, Full};
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{body::Incoming, Request, Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use ring::digest::{digest, SHA256};
    use tokio::net::TcpListener;
    use tokio_rustls::TlsAcceptor;

    use super::{authorization_scope, json, jwks, sign, URL_SAFE_NO_PAD};
    use crate::app::Cassie;
    use crate::config::CassieRuntimeConfig;
    use crate::rest::oidc::{now, provider, Oidc, Settings, MAX_CHALLENGES};
    use base64::Engine as _;

    struct BrowserRequest {
        nonce: String,
        challenge: String,
    }

    struct Issuer {
        base: String,
        public_keys: Vec<u8>,
        signing: aws_lc_rs::signature::RsaKeyPair,
        browser: OnceLock<BrowserRequest>,
        discoveries: AtomicUsize,
    }

    impl Issuer {
        async fn reply(&self, request: Request<Incoming>) -> Response<Full<bytes::Bytes>> {
            match request.uri().path() {
                "/.well-known/openid-configuration" => {
                    self.discoveries.fetch_add(1, Ordering::Relaxed);
                    Response::new(Full::new(
                        json!({
                            "issuer": self.base,
                            "authorization_endpoint": format!("{}authorize", self.base),
                            "token_endpoint": format!("{}token", self.base),
                            "jwks_uri": format!("{}jwks", self.base),
                            "token_endpoint_auth_methods_supported": ["client_secret_post"]
                        })
                        .to_string()
                        .into(),
                    ))
                }
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
            discoveries: AtomicUsize::new(0),
        });
        let mut settings = Settings::new(
            base,
            "cassie-client".into(),
            "fixture-secret".into(),
            "cassie-api".into(),
            "https://ui.example/api/v1/auth/oidc/callback".into(),
            "external_reader".into(),
            "permissions".into(),
        )
        .expect("issuer settings");
        settings.authorization_scope =
            authorization_scope("read:db profile").expect("request API scopes");
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

    fn cassie_fixture() -> Cassie {
        let config = CassieRuntimeConfig {
            password: uuid::Uuid::new_v4().to_string(),
            rest_external_https: true,
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
        let admin = cassie.create_session("root", Some("analytics".to_owned()));
        for sql in [
            "CREATE TABLE samples (id INTEGER PRIMARY KEY)",
            "INSERT INTO samples VALUES (7)",
            "GRANT USAGE ON SCHEMA public TO external_reader",
            "GRANT SELECT ON TABLE samples TO external_reader",
        ] {
            cassie
                .execute_sql(&admin, sql, vec![])
                .expect("grant fixture");
        }
        cassie
    }

    fn response_cookie(response: &reqwest::Response, name: &str) -> String {
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|header| header.to_str().ok())
            .find(|cookie| cookie.split_once('=').is_some_and(|(key, _)| key == name))
            .filter(|cookie| cookie.contains("; Secure") && cookie.contains("; HttpOnly"))
            .and_then(|cookie| cookie.split(';').next())
            .expect("secure cookie")
            .to_owned()
    }

    async fn begin_browser_login(client: &reqwest::Client, base: &str) -> (url::Url, String) {
        let response = client
            .get(format!("{base}/api/v1/auth/oidc/start?next=%2Fquery"))
            .send()
            .await
            .expect("begin browser login");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let browser = response_cookie(&response, "__Host-cassie_oidc");
        let location = response.headers()["location"]
            .to_str()
            .expect("redirect location");
        (
            url::Url::parse(location).expect("authorization URL"),
            browser,
        )
    }

    async fn browser_login(client: &reqwest::Client, issuer: &Issuer, base: &str) -> String {
        let (older_authorization, _) = begin_browser_login(client, base).await;
        let older_state = older_authorization
            .query_pairs()
            .find(|(name, _)| name == "state")
            .expect("older state")
            .1;
        let (authorization, browser) = begin_browser_login(client, base).await;
        let rejected = client
            .get(format!(
                "{base}/api/v1/auth/oidc/callback?code=obsolete&state={older_state}"
            ))
            .header("cookie", &browser)
            .send()
            .await
            .expect("reject older callback");
        assert_eq!(rejected.headers()["location"], "/login?oidc_error=1");
        assert!(!rejected.headers().contains_key("set-cookie"));
        let params = authorization.query_pairs().collect::<HashMap<_, _>>();
        let param = |name| {
            params
                .get(name)
                .expect("authorization parameter")
                .to_string()
        };
        assert_eq!(param("code_challenge_method"), "S256");
        assert_eq!(param("audience"), "cassie-api");
        assert_eq!(param("scope"), "openid read:db profile");
        assert!(issuer
            .browser
            .set(BrowserRequest {
                nonce: param("nonce"),
                challenge: param("code_challenge"),
            })
            .is_ok());
        let callback = format!(
            "{base}/api/v1/auth/oidc/callback?code=fixture-code&state={}",
            param("state"),
        );
        let completed = client
            .get(&callback)
            .header("cookie", &browser)
            .send()
            .await
            .expect("finish callback");
        assert_eq!(completed.status(), StatusCode::SEE_OTHER);
        assert_eq!(completed.headers()["location"], "/query");
        let session = response_cookie(&completed, "cassie_session");
        assert_eq!(
            response_cookie(&completed, "__Host-cassie_oidc"),
            "__Host-cassie_oidc="
        );
        let replay = client
            .get(callback)
            .header("cookie", browser)
            .send()
            .await
            .expect("replay callback");
        assert_eq!(replay.headers()["location"], "/login?oidc_error=1");
        assert!(!replay.headers().contains_key("set-cookie"));
        session
    }

    #[test]
    fn should_skip_discovery_when_pending_browser_sign_ins_fill_capacity() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        // Act
        let (rejected, discoveries) = runtime.block_on(async {
            let (oidc, issuer, provider_task) = start().await;
            {
                let mut store = oidc.store.lock().expect("challenge store");
                let now = now().expect("unix time");
                for _ in 0..MAX_CHALLENGES {
                    store.begin("/", now).expect("pending sign-in");
                }
            }
            let outcome = oidc.start("/").await;
            provider_task.abort();
            (outcome.is_err(), issuer.discoveries.load(Ordering::Relaxed))
        });

        // Assert
        assert!(rejected);
        assert_eq!(discoveries, 0);
    }

    async fn execute_samples_query(
        client: &reqwest::Client,
        base: &str,
        cookie: &str,
        database: &str,
    ) -> StatusCode {
        client
            .post(format!("{base}/api/v1/admin/query/execute"))
            .header("cookie", cookie)
            .json(&json!({"database": database, "sql": "SELECT id FROM samples"}))
            .send()
            .await
            .expect("query response")
            .status()
    }

    #[test]
    fn should_enforce_database_scope_through_rest_given_a_browser_login() {
        // Arrange
        let cassie = cassie_fixture();
        let observer = cassie.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        // Act
        let (methods, subject, databases, allowed, denied, logout, after_logout) = runtime
            .block_on(async {
                let (oidc, issuer, provider_task) = start().await;
                let listener = TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind Cassie REST");
                let base = format!(
                    "http://{}",
                    listener.local_addr().expect("Cassie REST address")
                );
                let rest_task = tokio::spawn(crate::rest::router::run_oidc_test_listener(
                    listener,
                    cassie,
                    Arc::new(oidc),
                ));
                let client = reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(std::time::Duration::from_secs(5))
                    .build()
                    .expect("browser");
                let methods = client
                    .get(format!("{base}/api/v1/auth/methods"))
                    .send()
                    .await
                    .expect("login methods")
                    .json::<serde_json::Value>()
                    .await
                    .expect("method JSON");
                let cookie = browser_login(&client, &issuer, &base).await;
                let session_url = format!("{base}/api/v1/auth/session");
                let subject = client
                    .get(&session_url)
                    .header("cookie", &cookie)
                    .send()
                    .await
                    .expect("browser session")
                    .json::<serde_json::Value>()
                    .await
                    .expect("session JSON");
                let databases = client
                    .get(format!("{base}/api/v1/admin/databases"))
                    .header("cookie", &cookie)
                    .send()
                    .await
                    .expect("database list")
                    .json::<serde_json::Value>()
                    .await
                    .expect("database JSON");
                let allowed = execute_samples_query(&client, &base, &cookie, "analytics").await;
                let denied = execute_samples_query(&client, &base, &cookie, "postgres").await;
                let logout = client
                    .post(format!("{base}/api/v1/auth/logout"))
                    .header("cookie", &cookie)
                    .send()
                    .await
                    .expect("logout")
                    .status();
                let after_logout = client
                    .get(&session_url)
                    .header("cookie", cookie)
                    .send()
                    .await
                    .expect("revoked session")
                    .status();
                rest_task.abort();
                provider_task.abort();
                (
                    methods,
                    subject,
                    databases,
                    allowed,
                    denied,
                    logout,
                    after_logout,
                )
            });

        // Assert
        assert_eq!(methods, json!({"oidc": true}));
        assert_eq!(subject["role"], "external_reader");
        assert!(subject["user"]
            .as_str()
            .is_some_and(|user| user.starts_with("oidc:")));
        assert_eq!(databases, json!([{"name": "analytics"}]));
        assert_eq!(allowed, StatusCode::OK);
        assert_eq!(denied, StatusCode::FORBIDDEN);
        assert_eq!(logout, StatusCode::OK);
        assert_eq!(after_logout, StatusCode::UNAUTHORIZED);
        assert_eq!(
            observer.metrics()["rest"]["blocking_completed_total"]["rest_oidc_role"],
            1
        );
    }
}
