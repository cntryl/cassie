use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieError};
use reqwest::StatusCode;
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

#[path = "support/pgwire.rs"]
mod support;

#[test]
fn should_apply_the_same_parser_complexity_limit_through_rest_pgwire_and_direct_sql() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("parser-transport-limit");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let direct_session = cassie.create_session("direct", None);
    let oversized = "x".repeat(1024 * 1024 + 1);
    let direct = cassie.execute_sql(&direct_session, &oversized, vec![]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    let (pgwire_fields, rest_status) = runtime.block_on(async {
        let pgwire = support::spawn_server(cassie.clone()).await;
        let mut socket = tokio::net::TcpStream::connect(pgwire.addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        write_half
            .write_all(&support::simple_query_frame(&oversized))
            .await
            .expect("write pgwire query");
        write_half.flush().await.expect("flush pgwire query");
        let frames = support::read_frames_until_ready(&mut reader).await;
        let pgwire_fields = support::parse_error_fields(
            &frames
                .iter()
                .find(|frame| frame.0 == b'E')
                .expect("pgwire resource error")
                .1,
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind REST");
        let rest_addr = listener.local_addr().expect("REST address");
        drop(listener);
        let shutdown = Arc::new(Notify::new());
        let rest = tokio::spawn(cassie::rest::router::run_with_shutdown(
            rest_addr.to_string(),
            cassie,
            Arc::clone(&shutdown),
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let client = reqwest::Client::new();
        let login = client
            .post(format!("http://{rest_addr}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "root",
                "password": "postgres"
            }))
            .send()
            .await
            .expect("REST login");
        let cookie = login
            .headers()
            .get("set-cookie")
            .expect("session cookie")
            .to_str()
            .expect("cookie text")
            .split(';')
            .next()
            .expect("cookie pair")
            .to_string();
        let rest_status = client
            .post(format!("http://{rest_addr}/api/v1/admin/query-executions"))
            .header("cookie", cookie)
            .json(&serde_json::json!({
                "database": "postgres",
                "sql": oversized
            }))
            .send()
            .await
            .expect("REST query")
            .status();
        shutdown.notify_waiters();
        let _ = rest.await;
        pgwire.stop().await;
        (pgwire_fields, rest_status)
    });

    // Act / Assert
    assert!(matches!(direct, Err(CassieError::ResourceLimit(_))));
    assert!(pgwire_fields
        .iter()
        .any(|(kind, value)| *kind == 'C' && value == "54000"));
    assert_eq!(rest_status, StatusCode::BAD_REQUEST);
    let _ = std::fs::remove_dir_all(path);
}
