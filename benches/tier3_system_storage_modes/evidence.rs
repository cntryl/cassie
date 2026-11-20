//! Pre-timing, non-timing evidence: EXPLAIN tokens and diagnostics counter
//! deltas. Tokens that do not exist yet are only asserted behind an explicit
//! opt-in and are never synthesized.

use std::collections::BTreeMap;

use cassie::types::Value;
use serde_json::{json, Map, Value as Json};

use super::Db;

pub type Tokens = BTreeMap<String, String>;

/// Parses `key=value` tokens from a textual EXPLAIN plan.
pub fn explain_tokens(db: &Db, sql: &str) -> Result<Tokens, String> {
    let rows = db.try_exec(&format!("EXPLAIN {sql}"))?;
    let Some(Value::String(plan)) = rows.first().and_then(|row| row.first()) else {
        return Err("EXPLAIN must return a textual plan".to_owned());
    };
    Ok(plan
        .split_ascii_whitespace()
        .filter_map(|token| {
            let (key, value) = token.split_once('=')?;
            Some((
                key.to_owned(),
                value
                    .trim_matches(|c: char| matches!(c, ',' | '|' | '[' | ']'))
                    .to_owned(),
            ))
        })
        .collect())
}

/// Asserts the arm reads the storage authority it is supposed to.
pub fn assert_storage_mode(tokens: &Tokens, expected: &str, query: &str) {
    assert_eq!(
        tokens.get("storage_mode").map(String::as_str),
        Some(expected),
        "{query}: EXPLAIN storage_mode"
    );
}

/// Asserts the vectorized-path tokens. These tokens do not exist yet, so this
/// runs only when `BENCH_TIER3_STORAGE_MODES_REQUIRE_VECTOR=1`; a missing token
/// then fails loudly instead of being assumed.
pub fn assert_vector_path(tokens: &Tokens, query: &str) {
    let token = |name: &str| {
        tokens
            .get(name)
            .unwrap_or_else(|| panic!("{query}: EXPLAIN token {name} is absent"))
    };
    assert_eq!(token("column_vector"), "eligible", "{query}: column_vector");
    let batches: u64 = token("column_vector_batches_delta")
        .parse()
        .unwrap_or_else(|_| panic!("{query}: column_vector_batches_delta must be numeric"));
    assert!(
        batches > 0,
        "{query}: column_vector_batches_delta must be > 0"
    );
    assert_eq!(
        token("row_documents_delta"),
        "0",
        "{query}: row_documents_delta"
    );
    assert_eq!(token("fallback_reason"), "none", "{query}: fallback_reason");
}

struct Snapshot {
    field_reads: cassie::midge::adapter::ColumnFieldReadSnapshot,
    validation_reads: u64,
    metrics: Json,
    memtable_bytes: Option<u64>,
}

fn snapshot(db: &Db) -> Snapshot {
    Snapshot {
        field_reads: db.cassie.midge.column_field_reads_for_diagnostics(),
        validation_reads: db.cassie.midge.column_validation_reads_for_diagnostics(),
        metrics: db.cassie.metrics(),
        memtable_bytes: db
            .cassie
            .midge
            .native_memory_diagnostics()
            .ok()
            .and_then(|state| u64::try_from(state.runtime.total_memtable_bytes).ok()),
    }
}

/// Numeric difference of two JSON objects, keeping only changed counters.
fn numeric_delta(before: &Json, after: &Json) -> Json {
    let mut out = Map::new();
    if let (Some(before), Some(after)) = (before.as_object(), after.as_object()) {
        for (key, value) in after {
            if let (Some(new), Some(old)) = (value.as_u64(), before.get(key).and_then(Json::as_u64))
            {
                if new != old {
                    out.insert(key.clone(), json!(new.saturating_sub(old)));
                }
            }
        }
    }
    Json::Object(out)
}

/// Executes the query once, untimed, and reports counter deltas. Peak query
/// memory is the process-wide high-water mark, not a per-query figure.
pub fn counted_run(db: &Db, sql: &str) -> Result<Json, String> {
    let before = snapshot(db);
    let rows = db.try_exec(sql)?;
    let after = snapshot(db);
    let reads = after.field_reads.since(before.field_reads);
    Ok(json!({
        "rows": rows.len(),
        "selected_field_reads": {
            "payload_reads": reads.payload_reads,
            "payload_bytes": reads.payload_bytes,
            "decodes": reads.decodes,
            "retained_bytes": reads.retained_bytes,
        },
        "ownership_validation_reads": after.validation_reads.saturating_sub(before.validation_reads),
        "storage_data_logical_reads": after.metrics["storage"]["data"]["reads"].as_u64().unwrap_or(0)
            .saturating_sub(before.metrics["storage"]["data"]["reads"].as_u64().unwrap_or(0)),
        "column_batches_delta": numeric_delta(&before.metrics["column_batches"], &after.metrics["column_batches"]),
        "peak_accounted_memory_bytes_process_high_water": after.metrics["query"]["peak_accounted_memory_bytes"],
        "memtable_bytes_before": before.memtable_bytes,
        "memtable_bytes_after": after.memtable_bytes,
    }))
}

/// The per-arm evidence record.
pub fn arm_record(tokens: &Tokens, counters: &Json) -> Json {
    let pick = |name: &str| tokens.get(name).cloned();
    json!({
        "tokens": {
            "storage_mode": pick("storage_mode"),
            "access_path": pick("access_path"),
            "fallback_reason": pick("fallback_reason"),
            "projection_pruning": pick("projection_pruning"),
            "scan_fields": pick("scan_fields"),
            "column_vector": pick("column_vector"),
            "column_vector_batches_delta": pick("column_vector_batches_delta"),
            "row_documents_delta": pick("row_documents_delta"),
        },
        "counters": counters,
    })
}

/// Total memtable bytes and storage-write operation count right now; the
/// amplification probe differences two of these.
pub fn write_counters(db: &Db) -> (u64, Option<u64>) {
    let writes = db.cassie.metrics()["storage"]["data"]["writes"]
        .as_u64()
        .unwrap_or(0);
    let memtable = db
        .cassie
        .midge
        .native_memory_diagnostics()
        .ok()
        .and_then(|state| u64::try_from(state.runtime.total_memtable_bytes).ok());
    (writes, memtable)
}
