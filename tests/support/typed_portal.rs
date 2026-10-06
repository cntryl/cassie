//! Typed materialized portal qualification using the shared wire fixtures.
use crate::support_pgwire as wire;
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use tokio::io::{AsyncRead, AsyncWrite, BufReader};

pub type Frames = Vec<(u8, Vec<u8>)>;

pub struct Pages {
    pub first: Frames,
    pub second: Frames,
    pub closed: Frames,
}

async fn exchange(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    frames: Vec<Vec<u8>>,
) -> Frames {
    wire::write_frames(writer, frames).await;
    wire::read_frames_until_ready(reader).await
}

pub fn resume_after_change(staged: bool, mutation: &str) -> Pages {
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("typed-portal");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let pages = runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("configuration");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie.execute_sql(&session, "CREATE TABLE typed_portal (id TEXT PRIMARY KEY, n BIGINT)", vec![]).expect("table");
        let documents = (0..6).map(|n| (Some(format!("doc-{n}")), serde_json::json!({"id": format!("doc-{n}"), "n": n}))).collect();
        cassie.midge.put_documents("typed_portal", documents).expect("seed");
        let observer = cassie.clone();
        let server = wire::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr).await.expect("connection");
        let (read_half, mut writer) = socket.split();
        let mut reader = BufReader::new(read_half);
        wire::complete_startup(&mut reader, &mut writer).await;
        wire::transaction_control(&mut reader, &mut writer, "BEGIN").await;
        if staged {
            let rows = exchange(&mut reader, &mut writer, vec![wire::simple_query_frame("INSERT INTO typed_portal (id, n) VALUES ('initial', 6)")]).await;
            assert_eq!(wire::error_code(&rows), None);
        }
        let first = exchange(&mut reader, &mut writer, vec![
            wire::parse_frame("typed_s", "SELECT CASE WHEN n IS NULL THEN NULL ELSE n END AS n FROM typed_portal WHERE n >= 0"),
            wire::bind_frame("typed_p", "typed_s", &[]),
            wire::execute_limited_frame("typed_p", 2), wire::sync_frame(),
        ]).await;
        if staged {
            let rows = exchange(&mut reader, &mut writer, vec![wire::simple_query_frame(mutation)]).await;
            assert_eq!(wire::error_code(&rows), None);
        } else {
            observer.execute_sql(&session, mutation, vec![]).expect("concurrent mutation");
        }
        let second = exchange(&mut reader, &mut writer, vec![wire::execute_limited_frame("typed_p", i32::MAX), wire::sync_frame()]).await;
        let closed = exchange(&mut reader, &mut writer, vec![
            wire::close_statement_frame("typed_s"), wire::close_statement_frame("typed_s"),
            wire::execute_frame("typed_p"), wire::sync_frame(),
        ]).await;
        drop(socket);
        server.stop().await;
        Pages { first, second, closed }
    });
    std::fs::remove_dir_all(path).expect("cleanup");
    pages
}
