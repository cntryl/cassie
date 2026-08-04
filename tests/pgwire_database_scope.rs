#[path = "support/pgwire.rs"]
mod pgwire_support;

use std::time::Duration;

use cassie::app::Cassie;
use pgwire_support::{
    data_dir, parse_error_fields, read_wire_frame, startup_frame, use_local_storage,
};

fn password_frame(password: &str) -> Vec<u8> {
    pgwire_support::password_message(password)
}

#[test]
fn should_report_3d000_for_missing_startup_database() {
    // Arrange
    use_local_storage();
    let path = data_dir("missing_database");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
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
        let startup = startup_frame("root", "missing_db");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("write startup");
        let (auth_tag, auth_payload) = read_wire_frame(&mut reader).await;
        assert_eq!(auth_tag, b'R');
        assert_eq!(
            i32::from_be_bytes(
                auth_payload[..4]
                    .try_into()
                    .expect("authentication request code")
            ),
            3
        );
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &password_frame("postgres"))
            .await
            .expect("write password");
        let (tag, payload) = read_wire_frame(&mut reader).await;
        let fields = parse_error_fields(&payload);

        // Assert
        assert_eq!(tag, b'E');
        assert!(fields.contains(&('C', "3D000".to_string())));

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
