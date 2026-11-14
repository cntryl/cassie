//! Untimed native-capacity and COPY outcome observations. Diagnostic probes do
//! not establish qualification and never retry a failed COPY automatically.

use super::{data, Db, FACT_ROWS};
use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use serde_json::{json, Value};

#[path = "readback.rs"]
mod readback;

pub const NATIVE_BUDGET: usize = 2 * 1024 * 1024 * 1024;
pub const MEMTABLE_LIMIT: usize = 512 * 1024 * 1024;
const COPY_BATCH_ROWS: usize = 5_000;

#[derive(Clone, Copy, Debug)]
pub(super) enum NativeProfile {
    Qualification,
    DefaultProbe,
}

impl NativeProfile {
    pub(super) fn configure(self, config: &mut CassieRuntimeConfig) {
        let (budget, memtable) = match self {
            Self::Qualification => (Some(NATIVE_BUDGET), Some(MEMTABLE_LIMIT)),
            Self::DefaultProbe => (None, None),
        };
        config.storage_memory_budget_bytes = budget;
        config.storage_memtable_size_bytes = memtable;
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum FixtureKind {
    Fact,
    Dimension,
}

#[derive(Clone, Copy)]
struct CopyBatch<'a> {
    table: &'a str,
    kind: FixtureKind,
    start: usize,
    end: usize,
    acknowledged: usize,
}

pub(super) fn native_state(db: &Db) -> Value {
    match db.cassie.midge.native_memory_diagnostics() {
        Ok(snapshot) => json!(snapshot),
        Err(error) => {
            json!({"configuration": db.cassie.midge.native_memory_configuration(), "runtime_error": error.to_string()})
        }
    }
}

impl Db {
    fn copy_fixture_batch(
        &self,
        batch: CopyBatch<'_>,
        payload: &str,
    ) -> Result<usize, CassieError> {
        println!(
            "storage_fixture={}",
            json!({"phase": "before_copy", "table": batch.table, "kind": format!("{:?}", batch.kind), "start": batch.start, "end": batch.end, "acknowledged_before": batch.acknowledged, "native": native_state(self)})
        );
        let statement = CopyStatement {
            table: batch.table.to_owned(),
            columns: vec![],
            format: CopyFormat::Csv,
            header: false,
        };
        let result = self
            .cassie
            .copy_from_csv_stdin(&self.session, &statement, payload.as_bytes());
        let native = native_state(self);
        match result {
            Ok(actual) => {
                println!(
                    "storage_fixture={}",
                    json!({"phase": "after_copy", "table": batch.table, "start": batch.start, "end": batch.end, "acknowledged_before": batch.acknowledged, "returned_rows": actual, "native": native})
                );
                assert_eq!(actual, batch.end - batch.start, "fixture COPY row count");
                Ok(actual)
            }
            Err(error) => {
                let readback = match readback::authoritative(
                    self,
                    batch.table,
                    batch.kind,
                    batch.acknowledged,
                    batch.end,
                ) {
                    Ok(value) => value,
                    Err(read_error) => json!({"readback_error": read_error.to_string()}),
                };
                println!(
                    "storage_fixture={}",
                    json!({"phase": "copy_error", "table": batch.table, "start": batch.start, "end": batch.end, "acknowledged_before": batch.acknowledged, "error": error.to_string(), "sql_state": error.sql_state(), "error_debug": format!("{error:?}"), "native": native, "authoritative_readback": readback, "native_after_readback": native_state(self), "automatic_retry": false})
                );
                Err(error)
            }
        }
    }

    pub(super) fn seed(&self) -> Result<(), CassieError> {
        let dims = data::dim_csv();
        for (fact, dim, mode) in [
            ("f_row", "d_row", data::Storage::Row),
            ("f_col", "d_col", data::Storage::Column),
        ] {
            self.exec(&data::create_fact_sql(fact, mode));
            let mut acknowledged = 0;
            for start in (0..FACT_ROWS).step_by(COPY_BATCH_ROWS) {
                let end = (start + COPY_BATCH_ROWS).min(FACT_ROWS);
                acknowledged += self.copy_fixture_batch(
                    CopyBatch {
                        table: fact,
                        kind: FixtureKind::Fact,
                        start,
                        end,
                        acknowledged,
                    },
                    &data::fact_csv(start..end),
                )?;
            }
            self.exec(&data::create_dim_sql(dim, mode));
            self.copy_fixture_batch(
                CopyBatch {
                    table: dim,
                    kind: FixtureKind::Dimension,
                    start: 0,
                    end: data::DIM_ROWS,
                    acknowledged: 0,
                },
                &dims,
            )?;
        }
        Ok(())
    }
}
