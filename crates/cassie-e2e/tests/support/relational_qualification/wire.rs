//! Actual frontend packets, with scoped server and connection ownership.
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{Fixture, Parameter};
use crate::support_pgwire as protocol;

type Frames = Vec<(u8, Vec<u8>)>;

pub fn cycle(sql: &str, parameters: &[Parameter], result_format: i16) -> Vec<Vec<u8>> {
    let payloads = parameters
        .iter()
        .map(|parameter| {
            parameter.hex.as_ref().map_or_else(
                || {
                    parameter
                        .utf8
                        .as_ref()
                        .map(|value| value.as_bytes().to_vec())
                },
                |hex| {
                    assert_eq!(hex.len() % 2, 0);
                    Some(
                        hex.as_bytes()
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|pair| {
                                u8::from_str_radix(
                                    std::str::from_utf8(pair).expect("literal hex"),
                                    16,
                                )
                                .expect("literal byte")
                            })
                            .collect::<Vec<_>>(),
                    )
                },
            )
        })
        .collect::<Vec<_>>();
    let values = payloads.iter().map(Option::as_deref).collect::<Vec<_>>();
    let oids = parameters
        .iter()
        .map(|parameter| parameter.oid)
        .collect::<Vec<_>>();
    let formats = parameters
        .iter()
        .map(|parameter| parameter.format)
        .collect::<Vec<_>>();
    vec![
        protocol::parse_frame_with_types("", sql, &oids),
        protocol::describe_statement_frame(""),
        protocol::bind_frame_with_formats("", "", &formats, &values, &[result_format]),
        protocol::describe_portal_frame(""),
        protocol::execute_frame(""),
        protocol::sync_frame(),
    ]
}

pub fn run_wire(fixture: &Fixture, cycles: Vec<Vec<Vec<u8>>>) -> Vec<Frames> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("qualification runtime");
    runtime.block_on(async {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("private address");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let shutdown = Arc::new(tokio::sync::Notify::new());
        let server = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
            address.to_string(),
            fixture.cassie.clone(),
            cassie::config::CassieRuntimeConfig::default(),
            shutdown.clone(),
        ));
        let started = Instant::now();
        let stream = loop {
            if let Ok(stream) = tokio::net::TcpStream::connect(address).await {
                break stream;
            }
            assert!(!server.is_finished(), "server stopped before accepting");
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "server readiness"
            );
            tokio::task::yield_now().await;
        };
        let (mut reader, mut writer) = stream.into_split();
        protocol::complete_startup(&mut reader, &mut writer).await;
        let mut results = Vec::new();
        for cycle in cycles {
            protocol::write_frames(&mut writer, cycle).await;
            results.push(
                protocol::read_frames_until_ready_within(&mut reader, Duration::from_secs(20))
                    .await,
            );
        }
        drop(writer);
        drop(reader);
        shutdown.notify_one();
        server.await.expect("server task").expect("server shutdown");
        let started = Instant::now();
        while Arc::strong_count(&fixture.cassie) != 1 {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "connection engine owner retirement"
            );
            tokio::task::yield_now().await;
        }
        results
    })
}
