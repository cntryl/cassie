use std::path::{Path, PathBuf};
use std::sync::Arc;

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::catalog::canonical_relation_name;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

use crate::fixture_dir::FixtureDir;

use super::model::{self, EventRow};

pub const TABLE: &str = "bench_mixed_time_series_events";
pub const DAILY_ROLLUP: &str = "bench_mixed_time_series_daily";
pub const RETENTION_POLICY: &str = "bench_mixed_time_series_retention";
pub const QUERY_MEMORY_BUDGET_BYTES: usize = 8 * 1024 * 1024 * 1024;
pub const FIXTURE_ROOT: &str = "/tmp";

pub struct Fixture {
    pub cassie: Arc<Cassie>,
    pub session: CassieSession,
    pub projection: String,
    pub data_dir: PathBuf,
    pub event_count: usize,
    pub device_count: usize,
    pub initial_rows: Arc<std::collections::BTreeMap<i64, EventRow>>,
    config: CassieRuntimeConfig,
    _fixture_dir: FixtureDir,
}

impl Fixture {
    pub fn seeded(
        scale: &str,
        event_count: usize,
        device_count: usize,
        query_memory_budget_bytes: usize,
    ) -> Self {
        let data_dir = Path::new(FIXTURE_ROOT).join(format!(
            "cassie-time-series-mixed-{scale}-{device_count}-{}",
            uuid::Uuid::new_v4()
        ));
        let fixture_dir = FixtureDir::new(data_dir.clone());
        let mut config = CassieRuntimeConfig::from_env().expect("benchmark runtime config");
        config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
        config.limits.max_result_rows = event_count.saturating_add(50_000);
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let (cassie, session) = open_store(&data_dir, config.clone());
        create_schema(&cassie, &session);
        let projection = canonical_relation_name("postgres", "public", TABLE);
        let initial_rows = Arc::new(model::initial_rows(event_count, device_count));
        seed_projection(&cassie, &projection, initial_rows.as_ref());
        create_maintenance_objects(&cassie, &session);

        Self {
            cassie,
            session,
            projection,
            data_dir,
            event_count,
            device_count,
            initial_rows,
            config,
            _fixture_dir: fixture_dir,
        }
    }

    /// Close the measured engine and open its committed store without reseeding.
    pub fn reopen(self) -> Self {
        let Self {
            cassie,
            session,
            projection,
            data_dir,
            event_count,
            device_count,
            initial_rows,
            config,
            _fixture_dir: fixture_dir,
        } = self;
        cassie.shutdown();
        drop(session);
        assert_eq!(
            Arc::strong_count(&cassie),
            1,
            "campaign workers must be joined"
        );
        drop(cassie);
        let (cassie, session) = open_store(&data_dir, config.clone());
        Self {
            cassie,
            session,
            projection,
            data_dir,
            event_count,
            device_count,
            initial_rows,
            config,
            _fixture_dir: fixture_dir,
        }
    }

    pub fn execute(&self, sql: &str) -> Result<Vec<Vec<Value>>, CassieError> {
        self.cassie
            .execute_sql(&self.session, sql, Vec::new())
            .map(|result| result.rows().to_vec())
    }
}

fn open_store(path: &Path, config: CassieRuntimeConfig) -> (Arc<Cassie>, CassieSession) {
    let cassie = Arc::new(
        Cassie::new_local_with_data_dir_and_config(path, config)
            .expect("open local APFS campaign store"),
    );
    cassie.startup().expect("start local campaign store");
    let session = cassie.create_session("benchmark", None);
    (cassie, session)
}

/// Table and fixed one-day time-series index exist before the seed so the
/// index is maintained by the seed commits themselves.
fn create_schema(cassie: &Cassie, session: &CassieSession) {
    for sql in [
        format!(
            "CREATE TABLE {TABLE} (id INT PRIMARY KEY, device TEXT NOT NULL, event_at TIMESTAMP NOT NULL, amount INT NOT NULL)"
        ),
        format!(
            "CREATE INDEX bench_mixed_time_series_time_idx ON {TABLE} USING time_series (event_at) WITH (bucket_width = '1 day', partition_by = device)"
        ),
    ] {
        cassie
            .execute_sql(session, &sql, Vec::new())
            .expect("create time-series mixed campaign schema");
    }
}

/// The rollup and the scheduled retention policy are created over the seeded
/// relation, so seeding is not dominated by per-batch rollup maintenance and the
/// measured windows start from a fully built rollup.
fn create_maintenance_objects(cassie: &Cassie, session: &CassieSession) {
    for sql in [
        format!(
            "CREATE ROLLUP {DAILY_ROLLUP} ON {TABLE} USING time_bucket('1 day',event_at) GROUP BY device AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum"
        ),
        format!(
            "CREATE RETENTION POLICY {RETENTION_POLICY} ON {TABLE} USING event_at RETAIN FOR '7 days'"
        ),
        format!(
            "ALTER RETENTION POLICY {RETENTION_POLICY} SCHEDULE DAILY START AT '{}'",
            model::RETENTION_AT
        ),
    ] {
        cassie
            .execute_sql(session, &sql, Vec::new())
            .expect("create time-series mixed campaign maintenance objects");
    }
}

fn seed_projection(
    cassie: &Cassie,
    projection: &str,
    rows: &std::collections::BTreeMap<i64, EventRow>,
) {
    for (batch_index, batch) in rows
        .values()
        .collect::<Vec<_>>()
        .chunks(model::WRITER_BATCH_EVENTS)
        .enumerate()
    {
        let mutations = batch
            .iter()
            .map(|row| {
                model::mutation(
                    (*row).clone(),
                    u64::try_from(row.id).expect("seed position fits u64"),
                    format!("seed-event-{}", row.id),
                )
            })
            .collect::<Vec<_>>();
        let replay = model::replay_batch(
            projection,
            &format!("seed-batch-{batch_index:06}"),
            &mutations,
        );
        cassie
            .replay_projection_batch_strict(replay)
            .expect("seed 128-event checkpointed writer batch");
    }
}
