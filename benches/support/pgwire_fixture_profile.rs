//! Phase measurements for fresh correctness-fixture lifecycles; no timing assertions.
use std::sync::Arc;
use std::time::{Duration, Instant};

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie_test_support::{support_pgwire as wire, support_sql};

#[derive(Clone, Copy)]
enum StartupHelper {
    Legacy,
    Owned,
}

impl StartupHelper {
    const fn label(self) -> &'static str {
        match self {
            Self::Legacy => "legacy_rebind_50ms",
            Self::Owned => "owned_listener",
        }
    }

    const fn delay_us(self) -> u32 {
        match self {
            Self::Legacy => 50_000,
            Self::Owned => 0,
        }
    }
}

async fn profile(sample: usize, helper: StartupHelper) {
    let started = Instant::now();
    let config = support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("memory fixture");
    cassie.startup().expect("startup");
    let fixture = started.elapsed();
    let started = Instant::now();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("owned listener");
    let addr = listener.local_addr().expect("listener address");
    let cassie = Arc::new(cassie);
    let server = match helper {
        StartupHelper::Legacy => {
            drop(listener);
            let server = tokio::spawn(cassie::pgwire::server::run(
                addr.to_string(),
                cassie,
                config,
            ));
            tokio::time::sleep(Duration::from_micros(u64::from(helper.delay_us()))).await;
            server
        }
        StartupHelper::Owned => tokio::spawn(cassie::pgwire::server::run_with_listener(
            listener, cassie, config,
        )),
    };
    let socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
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
    server.abort();
    let _ = server.await;
    let listener_stop = started.elapsed();
    println!(
        "{sample},{},{},{},{},{},{},{}",
        helper.label(),
        helper.delay_us(),
        fixture.as_micros(),
        connection.as_micros(),
        authentication.as_micros(),
        cycles.as_micros(),
        listener_stop.as_micros()
    );
}

fn main() {
    let samples = std::env::var("CASSIE_FIXTURE_PROFILE_SAMPLES").map_or(8, |value| {
        value.parse::<usize>().expect("positive sample count")
    });
    assert!(samples > 0, "sample count must be positive");
    println!("sample,helper,configured_startup_delay_us,fixture_us,listener_connect_us,authentication_us,cycles_us,listener_stop_us");
    for sample in 0..samples {
        let helpers = if sample % 2 == 0 {
            [StartupHelper::Legacy, StartupHelper::Owned]
        } else {
            [StartupHelper::Owned, StartupHelper::Legacy]
        };
        for helper in helpers {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            runtime.block_on(profile(sample, helper));
        }
    }
}
