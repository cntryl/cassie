//! Reported-only write amplification and on-disk footprint probes.
//!
//! Cassie exposes no Midge write-byte counter. Amplification therefore reports
//! the storage write-operation count (`storage.data.writes`) and the change in
//! total memtable bytes per logical row; the memtable figure is a proxy that
//! flushes can lower, so it is a lower bound, not an exact byte count.

use std::path::Path;
use std::sync::Arc;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use serde_json::{json, Value as Json};

use super::evidence::write_counters;
use super::{data, Db};

const COPY_BATCH_ROWS: usize = 5_000;
/// Local mode caps detached transaction validation at 8 MiB, so footprint
/// loads use smaller COPY batches.
const LOCAL_COPY_BATCH_ROWS: usize = 250;

fn measure_write<T>(db: &Db, logical_rows: usize, operation: impl FnOnce() -> T) -> Json {
    let (writes_before, memtable_before) = write_counters(db);
    let _ = operation();
    let (writes_after, memtable_after) = write_counters(db);
    let writes = writes_after.saturating_sub(writes_before);
    let rows = u64::try_from(logical_rows.max(1)).expect("rows");
    let memtable_delta = memtable_before
        .zip(memtable_after)
        .map(|(before, after)| i128::from(after) - i128::from(before));
    json!({
        "logical_rows": logical_rows,
        "storage_write_operations": writes,
        "storage_write_operations_per_row_x1000": writes.saturating_mul(1000) / rows,
        "memtable_bytes_delta": memtable_delta,
        "memtable_bytes_per_row": memtable_delta.map(|delta| delta / i128::from(rows)),
        "byte_counter": "memtable_delta_proxy",
    })
}

fn copy_rows(db: &Db, table: &str, rows: usize, batch: usize) {
    for start in (0..rows).step_by(batch) {
        let end = (start + batch).min(rows);
        let statement = CopyStatement {
            table: table.to_owned(),
            columns: vec![],
            format: CopyFormat::Csv,
            header: false,
        };
        let actual = db
            .cassie
            .copy_from_csv_stdin(
                &db.session,
                &statement,
                data::fact_csv(start..end).as_bytes(),
            )
            .unwrap_or_else(|error| panic!("probe COPY into {table} at [{start}, {end}): {error}"));
        assert_eq!(
            actual,
            end - start,
            "{table}: acknowledged COPY rows at [{start}, {end})"
        );
    }
}

/// COPY then UPDATE amplification on fresh heap and column tables.
pub fn write_amplification(db: &Db, rows: usize, local: bool) -> Json {
    let batch = if local {
        LOCAL_COPY_BATCH_ROWS
    } else {
        COPY_BATCH_ROWS
    };
    let updated = (rows / 10).clamp(1, 5_000);
    let mut report = serde_json::Map::new();
    for (arm, table, storage) in [
        ("heap", "amp_row", data::Storage::Row),
        ("column", "amp_col", data::Storage::Column),
    ] {
        db.exec(&data::create_fact_sql(table, storage));
        let copy = measure_write(db, rows, || copy_rows(db, table, rows, batch));
        let update = measure_write(db, updated, || {
            db.exec(&format!(
                "UPDATE {table} SET qty = qty + 1 WHERE id < {updated}"
            ))
        });
        report.insert(arm.to_owned(), json!({"copy": copy, "update": update}));
    }
    Json::Object(report)
}

fn directory_bytes(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => directory_bytes(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

fn footprint_arm(root: &Path, arm: &str, storage: data::Storage, rows: usize) -> Json {
    let dir = root.join(arm);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("footprint directory");
    let mut config = CassieRuntimeConfig::from_env().expect("benchmark config");
    config.limits.query_timeout_ms = 0;
    let bytes_while_open;
    {
        let cassie = Arc::new(
            Cassie::new_local_with_data_dir_and_config(&dir, config).expect("local Cassie"),
        );
        cassie.startup().expect("startup");
        let db = Db {
            session: cassie.create_session("benchmark", None),
            cassie,
        };
        db.exec(&data::create_fact_sql("footprint", storage));
        copy_rows(&db, "footprint", rows, LOCAL_COPY_BATCH_ROWS);
        bytes_while_open = directory_bytes(&dir);
    }
    json!({"arm": arm, "rows": rows, "bytes_after_load": bytes_while_open, "bytes_after_close": directory_bytes(&dir)})
}

/// One isolated local-mode sub-run per arm; each arm gets its own data
/// directory so the figures are whole-database footprints of one table.
pub fn disk_footprint(root: &Path, rows: usize) -> Json {
    json!({
        "storage_mode": "local",
        "heap": footprint_arm(root, "heap", data::Storage::Row, rows),
        "column": footprint_arm(root, "column", data::Storage::Column, rows),
    })
}
