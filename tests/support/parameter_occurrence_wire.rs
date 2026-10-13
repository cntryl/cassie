//! Observe the Parse+Flush reply before sending Sync, using the existing wire fixture.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract::{Cycle, Frames};
use cassie::app::Cassie;

pub enum Step {
    ParseFlush(Vec<u8>),
    ThroughReady(Cycle),
}

struct FixtureDirectory(String);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn run_script(label: &str, steps: Vec<Step>) -> Vec<Frames> {
    wire::use_local_storage();
    let path = wire::data_dir(label);
    let _directory = FixtureDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let batches = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
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
                Step::ParseFlush(parse) => {
                    wire::write_frames(&mut writer, vec![parse, wire::flush_frame()]).await;
                    let first = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        wire::read_wire_frame(&mut reader),
                    )
                    .await
                    .expect("Parse+Flush must reply before Sync");
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
