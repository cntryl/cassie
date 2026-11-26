use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::rotation_tests::{response, IssuerFixture};
use super::{claims, jwk, sign, signing_key, start_fixture_pgwire, tls_identities, ISSUER, ROLE};

fn run_portal_fixture(sql: &'static str) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(10), portal_fixture(sql))
                .await
                .expect("portal fixture hang guard");
        });
}

#[test]
fn should_release_suspended_materialized_portal_when_iam_binding_is_removed() {
    run_portal_fixture("SELECT value FROM observed ORDER BY value");
}

#[test]
fn should_release_suspended_streaming_portal_when_iam_binding_is_removed() {
    run_portal_fixture("SELECT value FROM observed");
}

async fn portal_fixture(sql: &str) {
    // Arrange
    let config = crate::config::CassieRuntimeConfig {
        pgwire_aws_iam_trust_file: Some("fixture-trust.json".into()),
        password: "test-root".into(),
        ..crate::config::CassieRuntimeConfig::default()
    };
    let cassie = Arc::new(crate::Cassie::new_in_memory_with_config(config.clone()).unwrap());
    cassie.startup().unwrap();
    let admin = cassie.create_session("root", None);
    for sql in [
        "CREATE ROLE service EXTERNAL LOGIN".to_owned(),
        format!("ALTER ROLE service BIND AWS IAM ROLE '{ROLE}' ISSUER '{ISSUER}'"),
        "CREATE TABLE observed (value INT)".to_owned(),
        "INSERT INTO observed VALUES (1), (2)".to_owned(),
        "GRANT USAGE ON SCHEMA public TO service".to_owned(),
        "GRANT SELECT ON TABLE observed TO service".to_owned(),
    ] {
        cassie.execute_sql(&admin, &sql, vec![]).unwrap();
    }
    let key = signing_key();
    let issuer = IssuerFixture::start(vec![response(&[jwk(&key)])]).await;
    let token = sign(&key, "fixture", &claims());
    let (server_tls, client_tls) = tls_identities();
    let (address, server) =
        start_fixture_pgwire(Arc::clone(&cassie), config, server_tls, &issuer, 1).await;
    let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
    socket
        .write_all(&[0, 0, 0, 8, 4, 210, 22, 47])
        .await
        .unwrap();
    assert_eq!(socket.read_u8().await.unwrap(), b'S');
    let mut socket = client_tls
        .connect(
            rustls::pki_types::ServerName::try_from("localhost").unwrap(),
            socket,
        )
        .await
        .unwrap();
    let startup = format!(
        "\0\x03\0\0user\0{}\0database\0postgres\0\0",
        crate::config::AWS_IAM_PGWIRE_SELECTOR
    );
    send(&mut socket, None, startup.as_bytes()).await;
    let (tag, auth) = read(&mut socket).await;
    assert_eq!((tag, auth.as_slice()), (b'R', [0, 0, 0, 3].as_slice()));
    send(&mut socket, Some(b'p'), format!("{token}\0").as_bytes()).await;
    read_until(&mut socket, b'Z').await;
    send(&mut socket, Some(b'Q'), b"BEGIN\0").await;
    read_until(&mut socket, b'Z').await;
    send(
        &mut socket,
        Some(b'P'),
        format!("stmt\0{sql}\0\0\0").as_bytes(),
    )
    .await;
    send(&mut socket, Some(b'B'), b"portal\0stmt\0\0\0\0\0\0\0").await;
    let execute = b"portal\0\0\0\0\x01";
    send(&mut socket, Some(b'E'), execute).await;
    send(&mut socket, Some(b'H'), &[]).await;
    assert_eq!(read_until(&mut socket, b's').await, 1);
    assert_eq!(cassie.metrics()["pgwire"]["portals"], 1);

    // Act
    cassie
        .execute_sql(
            &admin,
            &format!("ALTER ROLE service UNBIND AWS IAM ROLE '{ROLE}' ISSUER '{ISSUER}'"),
            vec![],
        )
        .unwrap();
    send(&mut socket, Some(b'E'), execute).await;
    send(&mut socket, Some(b'H'), &[]).await;

    // Assert
    let (tag, error) = read(&mut socket).await;
    assert_eq!(tag, b'E', "resume emits no further row or completion");
    assert!(
        error
            .split(|byte| *byte == 0)
            .any(|field| field == b"C28000"),
        "unexpected portal resume ErrorResponse: {}",
        String::from_utf8_lossy(&error)
    );
    assert_eq!(
        cassie.metrics()["pgwire"]["portals"],
        0,
        "release before Sync"
    );
    drop(socket);
    server.await.unwrap();
    assert_eq!(issuer.requests(), 1);
}

async fn send(socket: &mut (impl AsyncWriteExt + Unpin), tag: Option<u8>, body: &[u8]) {
    if let Some(tag) = tag {
        socket.write_u8(tag).await.unwrap();
    }
    socket
        .write_u32(u32::try_from(body.len() + 4).unwrap())
        .await
        .unwrap();
    socket.write_all(body).await.unwrap();
    socket.flush().await.unwrap();
}

async fn read(socket: &mut (impl AsyncReadExt + Unpin)) -> (u8, Vec<u8>) {
    let tag = socket.read_u8().await.unwrap();
    let size = socket.read_u32().await.unwrap().checked_sub(4).unwrap();
    let mut body = vec![0; usize::try_from(size).unwrap()];
    socket.read_exact(&mut body).await.unwrap();
    (tag, body)
}

async fn read_until(socket: &mut (impl AsyncReadExt + Unpin), end: u8) -> u32 {
    let mut rows = 0;
    loop {
        let (tag, body) = read(socket).await;
        assert_ne!(tag, b'E', "{}", String::from_utf8_lossy(&body));
        if tag == end {
            return rows;
        }
        rows += u32::from(tag == b'D');
    }
}
