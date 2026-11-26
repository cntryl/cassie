use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use tokio_rustls::TlsAcceptor;

use super::{claims, jwk, sign, signing_key, trust, ISSUER, NOW};
use crate::pgwire::iam::{client_builder, FixtureClock, Verifier};

const GUARD: Duration = Duration::from_secs(10);

pub(super) enum Reply {
    Body(Vec<u8>),
    Hold,
}

pub(super) struct IssuerFixture {
    pub(super) verifier: Arc<Verifier>,
    pub(super) clock: Arc<FixtureClock>,
    requests: Arc<AtomicUsize>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for IssuerFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

impl IssuerFixture {
    pub(super) async fn start(responses: Vec<Reply>) -> Self {
        let host = ISSUER.strip_prefix("https://").unwrap();
        let identity = rcgen::generate_simple_self_signed(vec![host.to_owned()]).unwrap();
        let private = rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(identity.signing_key.serialize_der()),
        );
        let server_tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![identity.cert.der().clone()], private)
            .unwrap();
        let trusted_fixture = reqwest::Certificate::from_der(identity.cert.der().as_ref()).unwrap();
        let acceptor = TlsAcceptor::from(Arc::new(server_tls));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            let observed = Arc::clone(&observed);
            let mut connections = tokio::task::JoinSet::new();
            for reply in responses {
                let (socket, _) = listener.accept().await.unwrap();
                let acceptor = acceptor.clone();
                let observed = Arc::clone(&observed);
                connections.spawn(async move {
                    let tls = acceptor.accept(socket).await.unwrap();
                    let reply = std::sync::Mutex::new(Some(reply));
                    let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                        let response = reply
                            .lock()
                            .unwrap()
                            .take()
                            .expect("one request per fixture connection");
                        let observed = Arc::clone(&observed);
                        async move {
                            assert_eq!(request.method(), hyper::Method::GET);
                            assert_eq!(request.uri().path(), "/.well-known/jwks.json");
                            observed.fetch_add(1, Ordering::SeqCst);
                            let body = match response {
                                Reply::Body(bytes) => Full::new(Bytes::from(bytes)),
                                Reply::Hold => std::future::pending().await,
                            };
                            Ok::<_, std::convert::Infallible>(Response::new(body))
                        }
                    });
                    let _ = http1::Builder::new()
                        .keep_alive(false)
                        .serve_connection(TokioIo::new(tls), service)
                        .await;
                });
            }
            while let Some(result) = connections.join_next().await {
                result.unwrap();
            }
        });
        let client = client_builder()
            .resolve(host, address)
            .tls_certs_only([trusted_fixture])
            .build()
            .unwrap();
        let clock = Arc::new(FixtureClock::new(NOW));
        Self {
            verifier: Arc::new(Verifier {
                trust: trust(),
                client,
                fixture_clock: Some(Arc::clone(&clock)),
            }),
            clock,
            requests,
            server,
        }
    }

    pub(super) fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

pub(super) fn response(keys: &[Value]) -> Reply {
    Reply::Body(serde_json::to_vec(&json!({"keys": keys})).unwrap())
}

fn run_fixture(fixture: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            tokio::time::timeout(GUARD, fixture)
                .await
                .expect("issuer fixture hang guard");
        });
}

#[test]
fn should_refresh_unknown_kid_at_the_bounded_boundary_and_retire_the_old_key() {
    // Arrange
    run_fixture(async {
        let old = signing_key();
        let new = signing_key();
        let old_token = sign(&old, "fixture", &claims());
        let new_token = sign(&new, "rotated", &claims());
        let mut rotated = jwk(&new);
        rotated["kid"] = json!("rotated");
        let fixture =
            IssuerFixture::start(vec![response(&[jwk(&old)]), response(&[rotated])]).await;

        // Act
        let initial = fixture.verifier.verify(&old_token).await;
        let early = fixture.verifier.verify(&new_token).await;
        let early_requests = fixture.requests();
        fixture
            .clock
            .set(NOW + super::super::REFRESH_INTERVAL.as_secs());
        let refreshed = fixture.verifier.verify(&new_token).await;
        let retired = fixture.verifier.verify(&old_token).await;

        // Assert
        assert!(initial.is_ok());
        assert!(early.is_err());
        assert_eq!(early_requests, 1);
        assert!(refreshed.is_ok());
        assert!(retired.is_err());
        assert_eq!(fixture.requests(), 2);
    });
}

#[test]
fn should_fail_closed_on_issuer_timeout_and_admit_a_later_refresh() {
    // Arrange
    run_fixture(async {
        let key = signing_key();
        let token = sign(&key, "fixture", &claims());
        let fixture = IssuerFixture::start(vec![Reply::Hold, response(&[jwk(&key)])]).await;

        // Act
        let timed_out = fixture.verifier.verify(&token).await;
        let throttled = fixture.verifier.verify(&token).await;
        let early_requests = fixture.requests();
        fixture
            .clock
            .set(NOW + super::super::REFRESH_INTERVAL.as_secs());
        let recovered = fixture.verifier.verify(&token).await;

        // Assert
        assert!(timed_out.is_err());
        assert!(throttled.is_err());
        assert_eq!(early_requests, 1);
        assert!(recovered.is_ok());
        assert_eq!(fixture.requests(), 2);
    });
}

#[test]
fn should_reject_issuer_response_and_key_count_over_their_limits() {
    // Arrange
    run_fixture(async {
        let key = signing_key();
        let token = sign(&key, "fixture", &claims());
        let mut many = vec![jwk(&key)];
        for i in 0..32 {
            let mut duplicate = jwk(&key);
            duplicate["kid"] = json!(format!("extra-{i}"));
            many.push(duplicate);
        }
        let oversized = Reply::Body(vec![b' '; super::super::MAX_JWKS_BYTES + 1]);
        let fixture = IssuerFixture::start(vec![oversized, response(&many)]).await;

        // Act
        let too_many_bytes = fixture.verifier.verify(&token).await;
        fixture
            .clock
            .set(NOW + super::super::REFRESH_INTERVAL.as_secs());
        let too_many_keys = fixture.verifier.verify(&token).await;

        // Assert
        assert!(too_many_bytes.is_err());
        assert!(too_many_keys.is_err());
        assert_eq!(fixture.requests(), 2);
    });
}
