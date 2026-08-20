use std::time::Duration;

use cassie::app::Cassie;

#[path = "support/pgwire.rs"]
mod pgwire_support;

use pgwire_support::{
    data_dir, parse_error_fields, password_message, startup_frame, use_local_storage,
};

const TEST_PASSWORD: &str = "cassie-pgwire-startup-password";

fn authenticated_config() -> cassie::config::CassieRuntimeConfig {
    cassie::config::CassieRuntimeConfig {
        password: TEST_PASSWORD.to_string(),
        ..cassie::config::CassieRuntimeConfig::default()
    }
}

fn startup_frame_with_params(user: &str, database: &str, params: &[(&str, &str)]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0");
    payload.extend_from_slice(user.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"database\0");
    payload.extend_from_slice(database.as_bytes());
    payload.push(0);
    for (key, value) in params {
        payload.extend_from_slice(key.as_bytes());
        payload.push(0);
        payload.extend_from_slice(value.as_bytes());
        payload.push(0);
    }
    payload.push(0);

    let mut frame = Vec::new();
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("startup payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn ssl_request_frame() -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&8_i32.to_be_bytes());
    frame.extend_from_slice(&80_877_103_i32.to_be_bytes());
    frame
}

async fn read_wire_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, i32, Vec<u8>) {
    let (tag, payload) = pgwire_support::read_wire_frame(reader).await;
    let len = i32::try_from(payload.len() + 4).expect("frame length must fit into i32");
    (tag, len, payload)
}

async fn complete_password_authentication(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
) -> (i32, i32) {
    let (challenge_tag, _, challenge_payload) = read_wire_frame(reader).await;
    assert_eq!(challenge_tag, b'R', "password challenge should use R tag");
    let challenge = i32::from_be_bytes(
        challenge_payload[..4]
            .try_into()
            .expect("authentication challenge code"),
    );
    tokio::io::AsyncWriteExt::write_all(writer, &password_message(TEST_PASSWORD))
        .await
        .expect("write password");
    let (authenticated_tag, _, authenticated_payload) = read_wire_frame(reader).await;
    assert_eq!(authenticated_tag, b'R', "authentication should use R tag");
    let authenticated = i32::from_be_bytes(
        authenticated_payload[..4]
            .try_into()
            .expect("authentication completion code"),
    );
    (challenge, authenticated)
}

async fn read_startup_error(addr: std::net::SocketAddr, startup: &[u8]) -> Vec<(char, String)> {
    let mut socket = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect pgwire");
    let (read_half, mut write_half) = socket.split();
    let mut reader = tokio::io::BufReader::new(read_half);
    tokio::io::AsyncWriteExt::write_all(&mut write_half, startup)
        .await
        .expect("write startup");
    let (tag, _, payload) = read_wire_frame(&mut reader).await;
    assert_eq!(tag, b'E', "invalid startup should return an error frame");
    parse_error_fields(&payload)
}

fn read_cstring(payload: &[u8], cursor: &mut usize) -> String {
    let tail = payload
        .get(*cursor..)
        .expect("cursor should be inside payload");
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .expect("cstring should be null terminated");
    let value = std::str::from_utf8(&tail[..end]).expect("cstring should be utf-8");
    *cursor += end + 1;
    value.to_string()
}

fn parse_parameter_status(payload: &[u8]) -> (String, String) {
    let mut cursor = 0usize;
    let key = read_cstring(payload, &mut cursor);
    let value = read_cstring(payload, &mut cursor);
    (key, value)
}

#[test]
fn should_validate_every_startup_parameter_regardless_of_user_value() {
    // Arrange
    use_local_storage();
    let path = data_dir("startup_parameter_validation");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = authenticated_config();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let cases = [
            ("", "", vec![], "invalid startup option 'database'"),
            (
                "",
                "postgres",
                vec![("replication", "database")],
                "unsupported startup option: replication",
            ),
            (
                "",
                "postgres",
                vec![("client_encoding", "LATIN1")],
                "invalid parameter value: invalid value for parameter \"client_encoding\": \"LATIN1\"",
            ),
            ("root", "", vec![], "invalid startup option 'database'"),
            (
                "root",
                "postgres",
                vec![("replication", "database")],
                "unsupported startup option: replication",
            ),
            (
                "root",
                "postgres",
                vec![("client_encoding", "LATIN1")],
                "invalid parameter value: invalid value for parameter \"client_encoding\": \"LATIN1\"",
            ),
        ];

        // Act
        let mut observed = Vec::new();
        for (user, database, params, expected) in cases {
            let startup = startup_frame_with_params(user, database, &params);
            let fields = read_startup_error(addr, &startup).await;
            let message = fields
                .iter()
                .find(|(field, _)| *field == 'M')
                .map(|(_, value)| value.clone());
            observed.push((message, expected));
        }

        // Assert
        for (actual, expected) in observed {
            assert_eq!(actual.as_deref(), Some(expected));
        }

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_support_binary_startup_with_password_authentication() {
    // Arrange
    use_local_storage();
    let path = data_dir("auth_ok");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = authenticated_config();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        let startup = startup_frame("root", "postgres");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("write startup");
        let authentication = complete_password_authentication(&mut reader, &mut write_half).await;

        // Assert
        assert_eq!(authentication, (3, 0));

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_emit_startup_parameter_statuses_after_password_authentication() {
    // Arrange
    use_local_storage();
    let path = data_dir("parameter_statuses");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = authenticated_config();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        let startup = startup_frame("root", "postgres");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("write startup");
        let authentication = complete_password_authentication(&mut reader, &mut write_half).await;
        let mut statuses = Vec::new();
        loop {
            let (tag, _len, payload) = read_wire_frame(&mut reader).await;
            if tag == b'Z' {
                break;
            }
            if tag == b'S' {
                statuses.push(parse_parameter_status(&payload));
            }
        }

        // Assert
        assert_eq!(authentication, (3, 0));
        assert!(statuses.contains(&("server_version".to_string(), "16.0".to_string())));
        assert!(statuses.contains(&("server_encoding".to_string(), "UTF8".to_string())));
        assert!(statuses.contains(&("client_encoding".to_string(), "UTF8".to_string())));
        assert!(statuses.contains(&("DateStyle".to_string(), "ISO, MDY".to_string())));
        assert!(statuses.contains(&("integer_datetimes".to_string(), "on".to_string())));
        assert!(statuses.contains(&("TimeZone".to_string(), "UTC".to_string())));
        assert!(statuses.contains(&("standard_conforming_strings".to_string(), "on".to_string())));

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_emit_backend_key_data_after_authentication() {
    // Arrange
    use_local_storage();
    let path = data_dir("backend_key_data");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = authenticated_config();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup_frame("root", "postgres"))
            .await
            .expect("write startup");
        complete_password_authentication(&mut reader, &mut write_half).await;

        // Act
        let mut backend_key = None;
        loop {
            let (tag, _, payload) = read_wire_frame(&mut reader).await;
            if tag == b'K' {
                backend_key = Some(payload);
            }
            if tag == b'Z' {
                break;
            }
        }

        // Assert
        let payload = backend_key.expect("backend key data");
        assert_eq!(payload.len(), 8);
        assert_ne!(i32::from_be_bytes(payload[..4].try_into().unwrap()), 0);
        assert_ne!(i32::from_be_bytes(payload[4..].try_into().unwrap()), 0);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_accept_libpq_startup_hints_with_password_authentication() {
    // Arrange
    use_local_storage();
    let path = data_dir("libpq_hints");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = authenticated_config();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        let startup = startup_frame_with_params(
            "root",
            "postgres",
            &[
                ("_pq_.libpq_version", "170000"),
                ("application_name", "sqlalchemy"),
            ],
        );
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("write startup");
        let authentication = complete_password_authentication(&mut reader, &mut write_half).await;

        // Assert
        assert_eq!(authentication, (3, 0));

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_not_supported_for_ssl_request() {
    // Arrange
    use_local_storage();
    let path = data_dir("ssl_request");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = authenticated_config();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");

        // Act
        let request = ssl_request_frame();
        tokio::io::AsyncWriteExt::write_all(&mut socket, &request)
            .await
            .expect("write ssl request");
        let mut reply = [0u8; 1];
        tokio::io::AsyncReadExt::read_exact(&mut socket, &mut reply)
            .await
            .expect("read ssl response");

        // Assert
        assert_eq!(reply[0], b'N', "SSL request should be explicitly declined");

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_error_when_password_does_not_match_for_cleartext_auth() {
    // Arrange
    use_local_storage();
    let path = data_dir("auth_failure");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = authenticated_config();
        config.password = "correct-password".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        let startup = startup_frame("root", "postgres");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("write startup");
        let (tag, _len, _payload) = read_wire_frame(&mut reader).await;
        assert_eq!(tag, b'R', "password challenge should use R tag");

        // password challenge for cleartext auth expects tag "R" status 3 then password message
        let payload = password_message("wrong-password");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &payload)
            .await
            .expect("write password");
        let (error_tag, _error_len, error_payload) = read_wire_frame(&mut reader).await;

        // Assert
        assert_eq!(
            error_tag, b'E',
            "auth failure should be returned as an error frame"
        );
        let error_fields = parse_error_fields(&error_payload);
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 'C')
                .map(|(_, value)| value.as_str()),
            Some("28000"),
            "error response should include SQL state"
        );
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 'S')
                .map(|(_, value)| value.as_str()),
            Some("FATAL"),
            "auth failure should be a fatal error"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
