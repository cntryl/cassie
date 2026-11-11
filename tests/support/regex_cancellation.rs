//! Real pgwire cancellation after the target regex has searched its input.

use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::support_pgwire as wire;

type Frames = Vec<(u8, Vec<u8>)>;

pub struct CancellationCase {
    pub control: Frames,
    pub canceled: Frames,
    pub recovery: Frames,
    pub memory_before: u64,
    pub memory_after: u64,
}

fn current_memory(cassie: &Cassie) -> u64 {
    cassie.metrics()["query"]["current_accounted_memory_bytes"]
        .as_u64()
        .expect("current accounted query memory")
}

pub fn run(sql: &str) -> CancellationCase {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread regex runtime");
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("regex runtime config"),
    );
    config.limits.query_timeout_ms = 0;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();

    runtime.block_on(async {
        let cassie = Arc::new(
            Cassie::new_in_memory_with_config(config.clone()).expect("memory regex instance"),
        );
        cassie.startup().expect("start regex instance");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("owned regex listener");
        let address = listener.local_addr().expect("regex listener address");
        let server = tokio::spawn(cassie::pgwire::server::run_with_listener(
            listener,
            Arc::clone(&cassie),
            config,
        ));
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("regex query connection");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
        let control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        assert_eq!(wire::error_code(&control), None, "regex positive control");
        let memory_before = current_memory(&cassie);

        let (_guard, selected, resume) = cassie::runtime::pause_next_query_after_regex_search();
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
        let matched =
            tokio::task::spawn_blocking(move || selected.recv_timeout(Duration::from_secs(5)))
                .await
                .expect("regex search observer task")
                .expect("target regex must search before cancellation");
        assert!(
            matched,
            "the target regex must report its completed positive match"
        );

        let mut cancel = tokio::net::TcpStream::connect(address)
            .await
            .expect("regex CancelRequest connection");
        cancel
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("send regex CancelRequest");
        cancel.shutdown().await.expect("finish regex CancelRequest");
        let mut response = [0; 1];
        let bytes = tokio::time::timeout(Duration::from_secs(5), cancel.read(&mut response))
            .await
            .expect("CancelRequest handling deadline")
            .expect("CancelRequest connection closes after handling");
        assert_eq!(bytes, 0, "CancelRequest sends no response body");
        resume
            .send(())
            .expect("resume regex after its completed search");

        let canceled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        let memory_after = current_memory(&cassie);
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        CancellationCase {
            control,
            canceled,
            recovery,
            memory_before,
            memory_after,
        }
    })
}
