use std::time::Duration;

use cassie::app::Cassie;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-pgwire-startup-{}-{}",
        label,
        uuid::Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

fn startup_frame(user: &str, database: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0");
    payload.extend_from_slice(user.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"database\0");
    payload.extend_from_slice(database.as_bytes());
    payload.push(0);
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

fn password_message(password: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(password.as_bytes());
    payload.push(0);

    let mut frame = Vec::new();
    frame.push(b'p');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("password frame size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

async fn read_wire_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, i32, Vec<u8>) {
    let mut header = [0u8; 5];
    tokio::io::AsyncReadExt::read_exact(reader, &mut header)
        .await
        .expect("read frame header");

    let tag = header[0];
    let len = i32::from_be_bytes(header[1..].try_into().expect("frame length"));
    let payload_len = usize::try_from(len - 4).expect("non-negative frame payload");
    let mut payload = vec![0u8; payload_len];
    if !payload.is_empty() {
        tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
            .await
            .expect("read frame payload");
    }

    (tag, len, payload)
}

#[test]
fn should_support_binary_startup_without_password() {
    // Arrange
    with_fallback();
    let path = data_dir("auth_ok");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let mut config = cassie::config::CassieRuntimeConfig::from_env();
        config.password.clear();
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
        let startup = startup_frame("postgres", "postgres");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("write startup");
        let (tag, _len, _payload) = read_wire_frame(&mut reader).await;

        // Assert
        assert_eq!(tag, b'R', "authentication response should use R tag");

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_not_supported_for_ssl_request() {
    // Arrange
    with_fallback();
    let path = data_dir("ssl_request");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let mut config = cassie::config::CassieRuntimeConfig::from_env();
        config.password.clear();
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
    with_fallback();
    let path = data_dir("auth_failure");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let mut config = cassie::config::CassieRuntimeConfig::from_env();
        config.password = "correct-password".to_string();

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
        let startup = startup_frame("postgres", "postgres");
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
        assert!(
            String::from_utf8_lossy(&error_payload).contains("28000"),
            "error response should include SQL state"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
