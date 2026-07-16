use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

#[path = "support/pgwire.rs"]
mod support;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

async fn connect_tls(
    address: std::net::SocketAddr,
    certificate: rustls::pki_types::CertificateDer<'static>,
) -> tokio_rustls::client::TlsStream<tokio::net::TcpStream> {
    let mut socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect pgwire");
    socket
        .write_all(&[0, 0, 0])
        .await
        .expect("fragmented SSLRequest prefix");
    tokio::task::yield_now().await;
    socket
        .write_all(&[8, 4, 210, 22, 47])
        .await
        .expect("fragmented SSLRequest suffix");
    let mut response = [0_u8; 1];
    socket
        .read_exact(&mut response)
        .await
        .expect("SSL response");
    assert_eq!(response, *b"S");
    let mut roots = RootCertStore::empty();
    roots.add(certificate).expect("root certificate");
    let client = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConnector::from(Arc::new(client))
        .connect(
            ServerName::try_from("localhost")
                .expect("server name")
                .to_owned(),
            socket,
        )
        .await
        .expect("TLS handshake")
}

#[test]
fn should_execute_pgwire_query_over_negotiated_tls() {
    // Arrange
    support::with_fallback();
    let data_path = support::data_dir("tls-query");
    let tls_path = std::env::temp_dir().join(format!("cassie-pgwire-tls-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tls_path).expect("TLS directory");
    let certificate_path = tls_path.join("cert.pem");
    let key_path = tls_path.join("key.pem");
    let identity =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("TLS identity");
    std::fs::write(&certificate_path, identity.cert.pem()).expect("certificate fixture");
    std::fs::write(&key_path, identity.key_pair.serialize_pem()).expect("key fixture");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_path).expect("cassie");
        cassie.startup().expect("startup");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let config = CassieRuntimeConfig {
            password: String::new(),
            pgwire_tls_cert_file: Some(certificate_path.to_string_lossy().to_string()),
            pgwire_tls_key_file: Some(key_path.to_string_lossy().to_string()),
            ..CassieRuntimeConfig::default()
        };
        let shutdown = Arc::new(tokio::sync::Notify::new());
        let server = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
            address.to_string(),
            Arc::new(cassie),
            config,
            shutdown.clone(),
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let tls = connect_tls(address, identity.cert.der().clone()).await;
        let (mut reader, mut writer) = tokio::io::split(tls);

        // Act
        support::complete_startup(&mut reader, &mut writer).await;
        writer
            .write_all(&support::simple_query_frame("SELECT 1 AS value"))
            .await
            .expect("query frame");
        let query = support::read_frames_until_ready(&mut reader).await;

        // Assert
        assert!(query.iter().any(|(tag, _)| *tag == b'D'));
        assert_eq!(
            query.last().map(|frame| frame.1.as_slice()),
            Some(b"I".as_slice())
        );

        shutdown.notify_waiters();
        server.await.expect("server task").expect("server shutdown");
    });

    let _ = std::fs::remove_dir_all(data_path);
    let _ = std::fs::remove_dir_all(tls_path);
}

#[test]
fn should_preserve_pgwire_cancel_protocol_over_tls() {
    // Arrange
    support::with_fallback();
    let data_path = support::data_dir("tls-cancel");
    let tls_path = std::env::temp_dir().join(format!("cassie-pgwire-tls-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tls_path).expect("TLS directory");
    let certificate_path = tls_path.join("cert.pem");
    let key_path = tls_path.join("key.pem");
    let identity =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).expect("TLS identity");
    std::fs::write(&certificate_path, identity.cert.pem()).expect("certificate fixture");
    std::fs::write(&key_path, identity.key_pair.serialize_pem()).expect("key fixture");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_path).expect("cassie");
        cassie.startup().expect("startup");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let config = CassieRuntimeConfig {
            password: String::new(),
            pgwire_tls_cert_file: Some(certificate_path.to_string_lossy().to_string()),
            pgwire_tls_key_file: Some(key_path.to_string_lossy().to_string()),
            ..CassieRuntimeConfig::default()
        };
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let query_tls = connect_tls(address, identity.cert.der().clone()).await;
        let (mut query_reader, mut query_writer) = tokio::io::split(query_tls);
        let (process_id, secret_key) =
            support::complete_startup_with_backend_key(&mut query_reader, &mut query_writer).await;

        // Act
        let mut cancel_tls = connect_tls(address, identity.cert.der().clone()).await;
        cancel_tls
            .write_all(&support::cancel_request_frame(process_id, secret_key))
            .await
            .expect("TLS cancel request");
        cancel_tls.shutdown().await.expect("close cancel request");
        query_writer
            .write_all(&support::simple_query_frame("SELECT 1"))
            .await
            .expect("query after idle cancel");
        let frames = support::read_frames_until_ready(&mut query_reader).await;

        // Assert
        assert!(frames.iter().all(|(tag, _)| *tag != b'E'));
        assert!(frames.iter().any(|(tag, _)| *tag == b'D'));

        server.abort();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(data_path);
    let _ = std::fs::remove_dir_all(tls_path);
}

#[test]
fn should_reject_invalid_pgwire_tls_identity_before_listening() {
    // Arrange
    support::with_fallback();
    let data_path = support::data_dir("tls-invalid-identity");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&data_path).expect("cassie");
        cassie.startup().expect("startup");
        let config = CassieRuntimeConfig {
            pgwire_tls_cert_file: Some("/tmp/cassie-missing-pgwire-cert.pem".to_string()),
            pgwire_tls_key_file: Some("/tmp/cassie-missing-pgwire-key.pem".to_string()),
            ..CassieRuntimeConfig::default()
        };

        // Act
        let error =
            cassie::pgwire::server::run("127.0.0.1:0".to_string(), Arc::new(cassie), config)
                .await
                .expect_err("invalid TLS identity must fail startup");

        // Assert
        assert!(error.to_string().contains("pgwire TLS"));
        assert!(error.to_string().contains("file not found"));
    });

    let _ = std::fs::remove_dir_all(data_path);
}
