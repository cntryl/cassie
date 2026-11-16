//! Phase measurements for fresh correctness-fixture lifecycles; no timing assertions.
use std::time::{Duration, Instant};

use cassie_test_support::{support_pgwire as wire, support_sql};

fn main() {
    let samples = std::env::var("CASSIE_FIXTURE_PROFILE_SAMPLES").map_or(8, |value| {
        value.parse::<usize>().expect("positive sample count")
    });
    assert!(samples > 0, "sample count must be positive");
    println!("sample,fixture_us,listener_connect_us,authentication_us,cycles_us,teardown_us");
    for sample in 0..samples {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let started = Instant::now();
            let cassie = support_sql::new_in_memory_cassie().expect("memory fixture");
            cassie.startup().expect("startup");
            let fixture = started.elapsed();
            let started = Instant::now();
            let server = wire::spawn_server(cassie).await;
            let socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect");
            let (mut reader, mut writer) = tokio::io::split(socket);
            let connection = started.elapsed();
            let started = Instant::now();
            wire::complete_startup(&mut reader, &mut writer).await;
            let authentication = started.elapsed();
            let started = Instant::now();
            for _ in 0..8 {
                wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 1")]).await;
                let frames =
                    wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
                assert_eq!(wire::data_rows(&frames), vec![vec![Some("1".into())]]);
            }
            let cycles = started.elapsed();
            let started = Instant::now();
            drop(reader);
            drop(writer);
            server.stop().await;
            let teardown = started.elapsed();
            println!(
                "{sample},{},{},{},{},{}",
                fixture.as_micros(),
                connection.as_micros(),
                authentication.as_micros(),
                cycles.as_micros(),
                teardown.as_micros()
            );
        });
    }
}
