use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::StorageFamily;

pub const COLLECTION: &str = "controlled_column_store";

pub struct Fixture {
    pub cassie: Cassie,
    pub session: CassieSession,
    payload_bytes: usize,
    column_store: bool,
}

impl Fixture {
    pub fn new(
        label: &str,
        budget: usize,
        workers: usize,
        rows: usize,
        payload_bytes: usize,
    ) -> Self {
        Self::create(label, budget, workers, rows, payload_bytes, true)
    }

    pub fn row_store(label: &str, budget: usize, rows: usize, payload_bytes: usize) -> Self {
        Self::row_store_with_workers(label, budget, 1, rows, payload_bytes)
    }

    pub fn row_store_with_workers(
        label: &str,
        budget: usize,
        workers: usize,
        rows: usize,
        payload_bytes: usize,
    ) -> Self {
        Self::create(label, budget, workers, rows, payload_bytes, false)
    }

    fn create(
        _label: &str,
        budget: usize,
        workers: usize,
        rows: usize,
        payload_bytes: usize,
        column_store: bool,
    ) -> Self {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = budget;
        config.limits.parallel_scan_workers = workers;
        config.limits.vectorized_joins_enabled = true;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_in_memory_with_config(config).expect("controlled Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        let suffix = if column_store {
            " WITH (storage = column_store)"
        } else {
            ""
        };
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {COLLECTION} (payload TEXT){suffix}"),
                vec![],
            )
            .expect("create ColumnStore table");
        for index in 0..rows {
            cassie
                .midge
                .put_document(
                    COLLECTION,
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({"payload": payload(index, payload_bytes)}),
                )
                .expect("seed ColumnStore row");
        }
        Self {
            cassie,
            session,
            payload_bytes,
            column_store,
        }
    }

    pub fn poison_payload(&self, index: usize) {
        let prefix = if self.column_store {
            self.cassie
                .midge
                .column_store_prefix_for_diagnostics(COLLECTION)
                .expect("ColumnStore prefix")
        } else {
            Vec::new()
        };
        let expected = payload(index, self.payload_bytes);
        let (key, _) = self
            .cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, &prefix)
            .expect("ColumnStore entries")
            .into_iter()
            .find(|(_, value)| {
                value
                    .windows(expected.len())
                    .any(|bytes| bytes == expected.as_bytes())
            })
            .expect("unique canary field");
        self.cassie
            .midge
            .raw_put(StorageFamily::Data, &key, &[u8::MAX])
            .expect("poison later compact field");
    }

    pub fn reads_since(&self, before: u64) -> u64 {
        self.cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before)
    }

    pub fn assert_cleanup(&self) {
        let metrics = self.cassie.metrics();
        assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
        assert_eq!(
            metrics["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
    }
}

/// Row `index`'s payload: its number, then `bytes` pseudo-random letters. Letter noise does not
/// shrink under column-store compression, so the stored field keeps these exact bytes for the
/// fixture to find and poison.
pub fn payload(index: usize, bytes: usize) -> String {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64 ^ (index as u64 + 1);
    let noise = (0..bytes)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            char::from(b'a' + u8::try_from(state % 26).expect("letter"))
        })
        .collect::<String>();
    format!("{index:04}-{noise}")
}
