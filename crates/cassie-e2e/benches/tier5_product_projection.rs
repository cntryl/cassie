#[path = "support/product_mixed.rs"]
mod product;
#[path = "support/workloads.rs"]
mod workloads;

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use product::{FixedRateReport, PgwireFixture};
use tokio::task::JoinSet;

const TABLE: &str = "product_projection_rows";

fn main() {
    let profile = product::profile();
    let output = product::output_dir();
    let windows = if profile.name == "smoke" { 1 } else { 3 };
    let mut window_reports = Vec::with_capacity(windows);
    for window in 0..windows {
        window_reports.push(run_window(profile, window));
    }
    let mut all_queries = Vec::new();
    let mut all_writes = Vec::new();
    let mut valid = true;
    for report in &window_reports {
        all_queries.extend_from_slice(&report.query_latency_us);
        all_writes.extend_from_slice(&report.writer.raw_latency_us);
        valid &= report.writer.scheduled == report.writer.offered
            && report.writer.generator_late == 0
            && report.writer.offered == report.writer.acknowledged
            && report.consistent_checkpoints > 0;
    }
    let p95 = product::percentile(&all_queries, 95);
    let acknowledged_rate = window_reports
        .iter()
        .map(|report| report.writer.acknowledged_at_window_end)
        .sum::<u64>();
    let acknowledged_rate = cassie::types::numeric::u64_to_f64(acknowledged_rate)
        / (cassie::types::numeric::usize_to_f64(windows) * profile.duration.as_secs_f64());
    let raw_artifact = "projection_lookup_raw.json";
    product::write_json(
        &output,
        raw_artifact,
        &serde_json::json!({"raw_reader_latency_us": &all_queries, "raw_writer_latency_us": &all_writes, "windows": &window_reports}),
    );
    product::write_json(
        &output,
        "projection_lookup.json",
        &serde_json::json!({
            "metric": "projection_lookup",
            "profile": profile.name,
            "measurement_valid": valid,
            "target_met": (profile.name == "full" && valid).then_some(p95 <= 5_000 && acknowledged_rate >= 1_000.0),
            "measurements": {
                "reader_p95_us": p95,
                "writer_p95_us": product::percentile(&all_writes, 95),
                "acknowledged_updates_per_second": acknowledged_rate,
                "reader_queries": all_queries.len(),
                "windows": window_reports.iter().map(|report| serde_json::json!({
                    "scheduled": report.writer.scheduled,
                    "offered": report.writer.offered,
                    "acknowledged": report.writer.acknowledged,
                    "acknowledged_at_window_end": report.writer.acknowledged_at_window_end,
                    "backlog_at_window_end": report.writer.backlog_at_window_end,
                    "drain_millis": report.writer.drain_millis,
                    "consistent_checkpoints": report.consistent_checkpoints,
                    "final_checkpoint": report.final_checkpoint,
                })).collect::<Vec<_>>(),
                "raw_artifact": raw_artifact,
            },
            "failures": if valid { Vec::<String>::new() } else { vec!["fixed-rate generator, writer acknowledgement, or checkpoint sample did not validate".to_owned()] },
        }),
    );
    assert!(
        valid,
        "projection workload evidence failed its correctness checks"
    );
}

#[derive(serde::Serialize)]
struct WindowReport {
    writer: FixedRateReport,
    query_latency_us: Vec<u64>,
    consistent_checkpoints: usize,
    final_checkpoint: i64,
}

fn run_window(profile: product::ProductProfile, window: usize) -> WindowReport {
    let runtime = workloads::runtime();
    let context = runtime
        .block_on(workloads::empty_disk_context_with_temp_budget(
            &format!("product-projection-{window}"),
            profile.seeded_rows.saturating_add(10_000),
            64 * 1024 * 1024,
        ))
        .expect("projection fixture");
    context
        .cassie
        .execute_sql(
            &context.session,
            &format!("CREATE TABLE {TABLE} (id INT PRIMARY KEY, email TEXT NOT NULL)"),
            Vec::new(),
        )
        .expect("create projection table");
    context
        .cassie
        .execute_sql(
            &context.session,
            "CREATE TABLE product_projection_checkpoint (position BIGINT NOT NULL)",
            Vec::new(),
        )
        .expect("create projector checkpoint table");
    context
        .cassie
        .execute_sql(
            &context.session,
            "INSERT INTO product_projection_checkpoint (position) VALUES (0)",
            Vec::new(),
        )
        .expect("seed projector checkpoint");
    seed_rows(&context, profile.seeded_rows);

    let fixture = runtime.block_on(PgwireFixture::start(Arc::clone(&context.cassie)));
    let clients = runtime.block_on(fixture.prepared_clients(
        "SELECT p.id, p.email, c.position FROM product_projection_rows p CROSS JOIN product_projection_checkpoint c WHERE p.id = $1",
        4,
    ));
    let cassie = Arc::clone(&context.cassie);
    let rows = profile.seeded_rows;
    let duration = profile.duration;
    let rate = 1_000 / profile.rate_scale;
    let writer = thread::spawn(move || {
        product::run_fixed_rate(rate, duration, |sequence| {
            let session = cassie.create_session("product-projector", None);
            let id = i64::try_from(sequence % u64::try_from(rows).expect("row count fits u64") + 1)
                .expect("row id fits i64");
            let position = i64::try_from(sequence + 1).expect("checkpoint fits i64");
            let email = format!("row-{id}-version-{position}");
            let sql = format!(
                "BEGIN; SET LOCAL cassie.projection_write_mode = 'replayable'; UPDATE {TABLE} SET email = '{email}' WHERE id = {id}; UPDATE product_projection_checkpoint SET position = {position}; COMMIT"
            );
            cassie
                .execute_sql(&session, &sql, Vec::new())
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    });
    let (query_latency_us, consistent_checkpoints) =
        runtime.block_on(run_readers(clients, duration));
    let writer = writer.join().expect("projection writer thread");
    let checkpoint_rows = context
        .cassie
        .execute_sql(
            &context.session,
            "SELECT position FROM product_projection_checkpoint",
            Vec::new(),
        )
        .expect("read final checkpoint");
    let final_checkpoint = checkpoint_rows.rows()[0][0]
        .as_i64()
        .expect("checkpoint integer");
    assert_eq!(
        final_checkpoint,
        i64::try_from(writer.acknowledged).expect("ack count fits i64")
    );
    runtime.block_on(fixture.shutdown());
    context.cassie.shutdown();
    WindowReport {
        writer,
        query_latency_us,
        consistent_checkpoints,
        final_checkpoint,
    }
}

fn seed_rows(context: &workloads::BenchContext, rows: usize) {
    for start in (1..=rows).step_by(500) {
        let end = (start + 500).min(rows + 1);
        let values = (start..end)
            .map(|id| format!("({id}, 'row-{id}-seed')"))
            .collect::<Vec<_>>()
            .join(",");
        context
            .cassie
            .execute_sql(
                &context.session,
                &format!("INSERT INTO {TABLE} (id,email) VALUES {values}"),
                Vec::new(),
            )
            .expect("seed projection rows");
    }
}

async fn run_readers(
    clients: Vec<(
        tokio_postgres::Client,
        tokio::task::JoinHandle<()>,
        tokio_postgres::Statement,
    )>,
    duration: Duration,
) -> (Vec<u64>, usize) {
    let stop = Instant::now() + duration;
    let mut readers = JoinSet::new();
    for (reader, (client, _connection, statement)) in clients.into_iter().enumerate() {
        readers.spawn(async move {
            let mut samples = Vec::new();
            let mut consistent = 0;
            let mut request = 0_i32;
            while Instant::now() < stop {
                let id = request % 128 + 1;
                let started = Instant::now();
                let rows = client
                    .query(&statement, &[&id])
                    .await
                    .expect("projection lookup");
                let elapsed = started.elapsed();
                assert_eq!(rows.len(), 1, "projection lookup returns one row");
                let email: String = rows[0].get(1);
                let checkpoint: i64 = rows[0].get(2);
                let version = email
                    .rsplit_once("version-")
                    .and_then(|(_, value)| value.parse::<i64>().ok())
                    .unwrap_or(0);
                assert!(
                    checkpoint >= version,
                    "row update and checkpoint are committed together"
                );
                consistent += 1;
                samples.push(u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX));
                request = request.wrapping_add(1 + i32::try_from(reader).unwrap_or_default());
            }
            (samples, consistent)
        });
    }
    let mut samples = Vec::new();
    let mut consistent = 0;
    while let Some(result) = readers.join_next().await {
        let (mut reader_samples, reader_consistent) = result.expect("pgwire reader task");
        samples.append(&mut reader_samples);
        consistent += reader_consistent;
    }
    (samples, consistent)
}
