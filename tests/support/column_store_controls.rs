use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::StorageFamily;

pub const COLLECTION: &str = "controlled_column_store";

pub struct Fixture {
    pub cassie: Cassie,
    pub session: CassieSession,
    path: String,
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
        label: &str,
        budget: usize,
        workers: usize,
        rows: usize,
        payload_bytes: usize,
        column_store: bool,
    ) -> Self {
        crate::support_sql::use_local_storage();
        let path = crate::support_sql::data_dir(label);
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = budget;
        config.limits.parallel_scan_workers = workers;
        config.limits.vectorized_joins_enabled = true;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("controlled Cassie");
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
            path,
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

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub fn payload(index: usize, bytes: usize) -> String {
    format!("{index:04}-{}", "x".repeat(bytes))
}
