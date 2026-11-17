//! A max-one listener retains its actual COPY WHERE worker after client teardown.
use super::*;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::types::Value;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn frame(socket: &mut tokio::net::TcpStream) -> (u8, Vec<u8>) {
    let tag = socket.read_u8().await.unwrap();
    let size = usize::try_from(socket.read_u32().await.unwrap()).unwrap();
    let mut body = vec![0; size - 4];
    socket.read_exact(&mut body).await.unwrap();
    (tag, body)
}

async fn send(socket: &mut tokio::net::TcpStream, tag: u8, body: &[u8]) {
    socket.write_all(&[tag]).await.unwrap();
    socket
        .write_all(&u32::try_from(body.len() + 4).unwrap().to_be_bytes())
        .await
        .unwrap();
    socket.write_all(body).await.unwrap();
}

async fn authenticate(socket: &mut tokio::net::TcpStream, password: &str) {
    let mut body = 196_608_u32.to_be_bytes().to_vec();
    body.extend_from_slice(b"user\0root\0database\0postgres\0\0");
    socket
        .write_all(&u32::try_from(body.len() + 4).unwrap().to_be_bytes())
        .await
        .unwrap();
    socket.write_all(&body).await.unwrap();
    assert_eq!(frame(socket).await.0, b'R');
    let mut password = password.as_bytes().to_vec();
    password.push(0);
    send(socket, b'p', &password).await;
    loop {
        let (tag, body) = frame(socket).await;
        assert_ne!(tag, b'E', "startup error {body:?}");
        if tag == b'Z' {
            break;
        }
    }
}

#[test]
fn should_complete_max_one_copy_where_and_retain_its_slot_through_disconnect_and_shutdown() {
    // Arrange
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let mut config = CassieRuntimeConfig {
                password: uuid::Uuid::new_v4().to_string(),
                ..CassieRuntimeConfig::default()
            };
            config.limits.pgwire_max_connections = 1;
            config.limits.max_query_workers = 1;
            let cassie = Arc::new(Cassie::new_in_memory_with_config(config.clone()).unwrap());
            cassie.startup().unwrap();
            let setup = cassie.create_session("root", None);
            cassie
                .execute_sql(&setup, "CREATE TABLE lease_copy_rows (id INT)", vec![])
                .unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let shutdown = Arc::new(tokio::sync::Notify::new());
            let server = tokio::spawn(crate::pgwire::server::run_with_listener_and_shutdown(
                listener,
                Arc::clone(&cassie),
                config.clone(),
                Arc::clone(&shutdown),
            ));
            let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
            tokio::time::timeout(
                Duration::from_secs(10),
                authenticate(&mut client, &config.password),
            )
            .await
            .unwrap();
            send(
                &mut client,
                b'Q',
                b"COPY lease_copy_rows FROM STDIN WHERE (id = 2)\0",
            )
            .await;
            let (tag, body) = tokio::time::timeout(Duration::from_secs(10), frame(&mut client))
                .await
                .unwrap();
            assert_eq!(tag, b'G', "COPY response {body:?}");
            let control = super::super::blocking::cleanup_test_hooks::install(&cassie, false);

            // Act
            send(&mut client, b'd', b"1\n2\n").await;
            send(&mut client, b'c', &[]).await;
            let observation = tokio::time::timeout(Duration::from_secs(10), control.entered)
                .await
                .unwrap()
                .unwrap();
            let admission = observation
                .connection_admission
                .expect("server worker captured its listener lease")
                .upgrade()
                .unwrap();
            let retained_before_disconnect = admission.try_acquire().is_err();
            drop(client);
            let mut rejected = tokio::net::TcpStream::connect(address).await.unwrap();
            let (tag, rejection) =
                tokio::time::timeout(Duration::from_secs(10), frame(&mut rejected))
                    .await
                    .unwrap();
            shutdown.notify_one();
            tokio::time::timeout(Duration::from_secs(10), server)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let retained_after_shutdown = admission.try_acquire().is_err();
            control.resume.send(()).unwrap();
            let released = tokio::time::timeout(
                Duration::from_secs(10),
                Arc::clone(&admission).acquire_owned(),
            )
            .await
            .unwrap()
            .unwrap();
            control.completed.await.unwrap();
            let rows = cassie
                .execute_sql(&setup, "SELECT id FROM lease_copy_rows", vec![])
                .unwrap();

            // Assert
            assert!(retained_before_disconnect);
            assert!(retained_after_shutdown);
            assert_eq!(tag, b'E');
            assert!(rejection
                .split(|byte| *byte == 0)
                .any(|field| field == b"C53300"));
            assert_eq!(released.num_permits(), 1);
            assert_eq!(rows.rows(), &[vec![Value::Int64(2)]]);
        });
}
