//! Bounded pgwire query cycles and success assertions shared by SQL and codec suites.

use crate::support_pgwire as wire;
use cassie::app::Cassie;
use cassie::types::Value;

pub type Frames = Vec<(u8, Vec<u8>)>;

pub type Cycle = Vec<Vec<u8>>;

pub fn run_cycles(label: &str, setup: &[(&str, Vec<Value>)], cycles: Vec<Cycle>) -> Vec<Frames> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let batches = runtime.block_on(async {
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        if config.password.is_empty() {
            crate::support_sql::test_root_password().clone_into(&mut config.password);
        }
        let cassie = Cassie::new_in_memory_with_config(config)
            .unwrap_or_else(|error| panic!("create ephemeral fixture {label}: {error}"));
        cassie.startup().expect("startup before table creation");
        let session = cassie.create_session("tester", None);
        for (sql, params) in setup {
            cassie
                .execute_sql(&session, sql, params.clone())
                .expect("seed finite metadata fixture");
        }
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        let mut batches = Vec::new();
        for cycle in cycles {
            wire::write_frames(&mut writer, cycle).await;
            batches.push(
                wire::read_frames_until_ready_within(
                    &mut reader,
                    std::time::Duration::from_secs(5),
                )
                .await,
            );
        }
        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    });
    batches
}

pub fn assert_success(frames: &[(u8, Vec<u8>)]) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire errors: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}
