//! Observe a Parse or Bind reply after Flush, before sending Sync.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract::{Cycle, Frames};
use cassie::app::Cassie;

pub enum Step {
    ParseFlush(Vec<u8>),
    BindFlush(Vec<u8>),
    ThroughReady(Cycle),
}

pub fn run_script(label: &str, steps: Vec<Step>) -> Vec<Frames> {
    let config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    run_script_with_config(label, steps, config)
}

pub fn run_script_with_config(
    label: &str,
    steps: Vec<Step>,
    config: cassie::config::CassieRuntimeConfig,
) -> Vec<Frames> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let batches = runtime.block_on(async move {
        let cassie = Cassie::new_in_memory_with_config(config)
            .unwrap_or_else(|error| panic!("create ephemeral fixture {label}: {error}"));
        cassie.startup().expect("startup occurrence fixture");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        let mut batches = Vec::new();
        for step in steps {
            let frames = match step {
                Step::ParseFlush(frame) | Step::BindFlush(frame) => {
                    wire::write_frames(&mut writer, vec![frame, wire::flush_frame()]).await;
                    let first = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        wire::read_wire_frame(&mut reader),
                    )
                    .await
                    .expect("frontend message plus Flush must reply before Sync");
                    vec![first]
                }
                Step::ThroughReady(cycle) => {
                    wire::write_frames(&mut writer, cycle).await;
                    wire::read_frames_until_ready_within(
                        &mut reader,
                        std::time::Duration::from_secs(5),
                    )
                    .await
                }
            };
            batches.push(frames);
        }
        drop(reader);
        drop(writer);
        server.stop().await;
        batches
    });
    batches
}
