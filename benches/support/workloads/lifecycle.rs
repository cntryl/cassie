use std::path::PathBuf;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;

use super::context::{
    disk_context_with_temp_budget, BenchContext, ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
};
use super::scaling::assert_scaling_cassie_resource_bounds;

pub struct StartupFixture {
    data_dir: PathBuf,
    expected_rows: usize,
    cleaned: bool,
}

impl StartupFixture {
    /// Builds and closes the durable fixture outside the measured reopen path.
    pub fn new(label: &str, expected_rows: usize) -> Result<Self, CassieError> {
        let context = disk_context_with_temp_budget(
            label,
            expected_rows,
            ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
        )
        .into_inner()?;
        Ok(Self::from_context(context, expected_rows))
    }

    #[must_use]
    pub fn from_context(context: BenchContext, expected_rows: usize) -> Self {
        assert_persisted_boundaries(&context.cassie, expected_rows);
        let data_dir = context.data_dir.clone();
        context.cassie.shutdown();
        drop(context);
        Self {
            data_dir,
            expected_rows,
            cleaned: false,
        }
    }

    #[must_use]
    pub fn reopen(&self) -> usize {
        let mut config = CassieRuntimeConfig::from_env().expect("startup benchmark config");
        config.limits.execution_result_cache_enabled =
            cassie::config::ExecutionResultCacheEnabled::disabled();
        config.limits.query_memory_budget_bytes = ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
        let cassie = Cassie::new_with_data_dir_and_config(self.data_dir.clone(), config)
            .expect("reopen benchmark database");
        cassie.startup().expect("startup benchmark database");
        assert_hydrated_catalog(&cassie, self.expected_rows);
        assert_persisted_boundaries(&cassie, self.expected_rows);
        assert_scaling_cassie_resource_bounds(&cassie);
        cassie.shutdown();
        std::hint::black_box(self.expected_rows)
    }

    /// Removes the owned benchmark data directory and reports cleanup failures.
    pub fn cleanup(mut self) -> Result<(), CassieError> {
        self.remove_data_dir()
            .map_err(|error| CassieError::Execution(error.to_string()))
    }

    fn remove_data_dir(&mut self) -> std::io::Result<()> {
        if self.data_dir.is_dir() {
            std::fs::remove_dir_all(&self.data_dir)?;
        } else if self.data_dir.exists() {
            std::fs::remove_file(&self.data_dir)?;
        }
        if self.data_dir.exists() {
            return Err(std::io::Error::other(format!(
                "benchmark data path still exists after cleanup: {}",
                self.data_dir.display()
            )));
        }
        self.cleaned = true;
        Ok(())
    }
}

fn assert_hydrated_catalog(cassie: &Cassie, expected_rows: usize) {
    assert!(
        cassie.catalog.get_schema("bench_documents").is_some(),
        "reopened fixture schema must be hydrated"
    );
    assert!(
        cassie
            .catalog
            .get_index("bench_documents", "bench_documents_status_score_idx")
            .is_some(),
        "reopened fixture index must be hydrated"
    );
    let actual_rows = cassie
        .catalog
        .get_cardinality_stats("bench_documents")
        .and_then(|stats| usize::try_from(stats.row_count).ok())
        .expect("reopened fixture cardinality must be hydrated");
    assert_eq!(actual_rows, expected_rows, "reopened fixture cardinality");
}

fn assert_persisted_boundaries(cassie: &Cassie, expected_rows: usize) {
    assert!(
        expected_rows > 0,
        "startup benchmark fixture must not be empty"
    );
    for id in ["doc-0".to_string(), format!("doc-{}", expected_rows - 1)] {
        assert!(
            cassie
                .midge
                .get_document("bench_documents", &id)
                .expect("read startup benchmark fixture boundary")
                .is_some(),
            "startup benchmark fixture must contain boundary document '{id}'"
        );
    }
}

impl Drop for StartupFixture {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.remove_data_dir();
        }
    }
}
