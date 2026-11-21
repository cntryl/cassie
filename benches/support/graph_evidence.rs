//! Deterministic counter evidence for graph reads (#1355). Each measured case runs on its own
//! freshly opened in-memory database with result caching disabled: a cold run (the first
//! query after open), a warm run, the `EXPLAIN` plan, and an exact full read when one is
//! available. Answers are compared with an oracle computed in plain Rust over the generated
//! edges. Nothing here is timed. Records go to `target/graph-evidence/<suite>.json`.

use std::path::PathBuf;

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;
use serde_json::{json, Value as Json};

/// The query memory budget every graph case runs under.
pub const BUDGET_BYTES: u64 = 64 * 1024 * 1024;

/// Counter totals of one database, or the difference between two snapshots.
#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub index_entries: u64,
    pub row_fetches: u64,
    pub full_rows: u64,
    pub validation_rows: u64,
    pub edges_admitted: u64,
    pub neighborhoods: u64,
    pub statement_cache_hits: u64,
    pub overlay_changes: u64,
    pub storage_reads: u64,
    pub reference_reads: u64,
    pub result_cache_hits: u64,
    pub result_cache_misses: u64,
}

impl Counts {
    /// Reads every counter of `cassie`.
    ///
    /// # Panics
    ///
    /// Panics when a counter is missing from the metrics document.
    #[must_use]
    pub fn snapshot(cassie: &Cassie) -> Self {
        let metrics = cassie.metrics();
        let read = |path: &Json, name: &str| {
            path[name]
                .as_u64()
                .unwrap_or_else(|| panic!("metric {name} is missing"))
        };
        let paths = &metrics["read_paths"];
        let cache = &metrics["execution_result_cache"];
        Self {
            index_entries: read(paths, "graph_index_entries"),
            row_fetches: read(paths, "graph_row_fetches"),
            full_rows: read(paths, "graph_full_rows"),
            validation_rows: read(paths, "graph_validation_rows"),
            edges_admitted: read(paths, "graph_edges_admitted"),
            neighborhoods: read(paths, "graph_neighborhoods_expanded"),
            statement_cache_hits: read(paths, "graph_cache_hits"),
            overlay_changes: read(paths, "graph_overlay_changes"),
            storage_reads: read(&metrics["storage"]["data"], "reads"),
            reference_reads: cassie.midge.graph_reference_reads_for_diagnostics(),
            result_cache_hits: read(cache, "hits"),
            result_cache_misses: read(cache, "misses"),
        }
    }

    #[must_use]
    pub fn since(self, before: Self) -> Self {
        Self {
            index_entries: self.index_entries - before.index_entries,
            row_fetches: self.row_fetches - before.row_fetches,
            full_rows: self.full_rows - before.full_rows,
            validation_rows: self.validation_rows - before.validation_rows,
            edges_admitted: self.edges_admitted - before.edges_admitted,
            neighborhoods: self.neighborhoods - before.neighborhoods,
            statement_cache_hits: self.statement_cache_hits - before.statement_cache_hits,
            overlay_changes: self.overlay_changes - before.overlay_changes,
            storage_reads: self.storage_reads - before.storage_reads,
            reference_reads: self.reference_reads - before.reference_reads,
            result_cache_hits: self.result_cache_hits - before.result_cache_hits,
            result_cache_misses: self.result_cache_misses - before.result_cache_misses,
        }
    }

    /// Graph storage reads: scalar index entries, point row reads and rows of whole mappings.
    #[must_use]
    pub fn graph_storage_reads(&self) -> u64 {
        self.index_entries + self.row_fetches + self.full_rows
    }

    #[must_use]
    pub fn to_json(self) -> Json {
        json!({
            "validated_edges": self.validation_rows,
            "expanded_vertices": self.neighborhoods,
            "expanded_edges": self.edges_admitted,
            "graph_storage_reads": self.graph_storage_reads(),
            "index_entries": self.index_entries,
            "row_fetches": self.row_fetches,
            "full_rows": self.full_rows,
            "statement_cache_hits": self.statement_cache_hits,
            "overlay_changes": self.overlay_changes,
            "generic_storage_reads": self.storage_reads,
            "reference_reads": self.reference_reads,
            "result_cache_hits": self.result_cache_hits,
            "result_cache_misses": self.result_cache_misses,
        })
    }
}

/// The canonical text of one result value: integral numbers print without a fraction.
#[must_use]
pub fn canonical(value: &Value) -> String {
    let text = match value {
        Value::Int64(number) => number.to_string(),
        Value::Float64(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Typed(typed) => typed.text(),
        Value::Null => "NULL".to_owned(),
        _ => "unsupported".to_owned(),
    };
    match text.parse::<f64>() {
        Ok(number) if number.fract() == 0.0 && number.abs() < 1e15 => format!("{number:.0}"),
        _ => text,
    }
}

/// Result rows as sorted canonical text, comparable with an oracle's rows.
#[must_use]
pub fn canonical_rows(rows: &[Vec<Value>]) -> Vec<Vec<String>> {
    let mut rows = rows
        .iter()
        .map(|row| row.iter().map(canonical).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

/// Single-column oracle rows from integers, sorted.
#[must_use]
pub fn int_rows(values: impl IntoIterator<Item = i64>) -> Vec<Vec<String>> {
    let mut rows = values
        .into_iter()
        .map(|value| vec![value.to_string()])
        .collect::<Vec<_>>();
    rows.sort();
    rows
}

/// One measured case: its statement, its oracle answer and its pinned fixture identity.
pub struct Spec<'a> {
    pub family: &'a str,
    pub case: &'a str,
    pub sql: &'a str,
    pub params: &'a [Value],
    pub expected: Vec<Vec<String>>,
    /// Whether `set_graph_exact_reads` applies (`GRAPH_TABLE`, not the weighted function).
    pub exact_switch: bool,
    pub fixture: Json,
}

struct Run {
    rows: Vec<Vec<String>>,
    counts: Counts,
}

fn run(cassie: &Cassie, session: &CassieSession, spec: &Spec<'_>) -> Result<Run, String> {
    let before = Counts::snapshot(cassie);
    let result = cassie
        .execute_sql(session, spec.sql, spec.params.to_vec())
        .map_err(|error| error.sql_state().to_string())?;
    Ok(Run {
        rows: canonical_rows(result.rows()),
        counts: Counts::snapshot(cassie).since(before),
    })
}

fn peak_bytes(cassie: &Cassie) -> u64 {
    cassie.metrics()["query"]["peak_accounted_memory_bytes"]
        .as_u64()
        .expect("peak accounted query memory")
}

fn explain(cassie: &Cassie, session: &CassieSession, spec: &Spec<'_>) -> String {
    let plan = cassie
        .execute_sql(
            session,
            &format!("EXPLAIN {}", spec.sql),
            spec.params.to_vec(),
        )
        .unwrap_or_else(|error| panic!("graph EXPLAIN failed with SQLSTATE {}", error.sql_state()));
    match plan.rows().first().and_then(|row| row.first()) {
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

/// The reason an `EXPLAIN` graph node gives for an exact mapping, when it names one.
fn exact_reason(plan: &str) -> Option<String> {
    let start = plan.find("=exact(")? + "=exact(".len();
    let end = plan[start..].find(')')?;
    Some(plan[start..start + end].to_owned())
}

/// What answered the cold run, from its counters and the plan: an index, a full-scan
/// fallback, or a cached result. Only `indexed` counts as indexed evidence.
fn answered_by(cold: &Counts, plan: &str) -> &'static str {
    if cold.result_cache_hits > 0 {
        "result_cache"
    } else if cold.full_rows > 0 || plan.contains("=exact(") {
        "exact_full_scan"
    } else if cold.index_entries > 0 {
        "indexed"
    } else {
        "unclassified"
    }
}

/// The records of one benchmark owner and the findings they raised.
pub struct Evidence {
    suite: &'static str,
    records: Vec<Json>,
    findings: Vec<String>,
}

impl Evidence {
    #[must_use]
    pub fn new(suite: &'static str) -> Self {
        Self {
            suite,
            records: Vec::new(),
            findings: Vec::new(),
        }
    }

    pub fn finding(&mut self, text: String) {
        eprintln!("graph_evidence_finding={text}");
        self.findings.push(text);
    }

    /// Runs the cold and warm queries, the plan and the exact comparison of `spec` on the
    /// database `cassie`, which must be freshly opened with result caching disabled. Returns
    /// the cold and warm counters.
    ///
    /// # Panics
    ///
    /// Panics when the indexed query fails.
    pub fn collect(
        &mut self,
        cassie: &Cassie,
        session: &CassieSession,
        spec: &Spec<'_>,
    ) -> (Counts, Counts) {
        let after_setup = peak_bytes(cassie);
        let cold = run(cassie, session, spec)
            .unwrap_or_else(|state| panic!("cold graph query failed with SQLSTATE {state}"));
        let after_cold = peak_bytes(cassie);
        let warm = run(cassie, session, spec)
            .unwrap_or_else(|state| panic!("warm graph query failed with SQLSTATE {state}"));
        let after_warm = peak_bytes(cassie);
        let plan = explain(cassie, session, spec);
        let answered = answered_by(&cold.counts, &plan);
        let fallback = exact_reason(&plan);
        let label = format!("{}/{}", spec.family, spec.case);
        let indexed_matches = cold.rows == spec.expected && warm.rows == spec.expected;
        if !indexed_matches {
            self.finding(format!("{label}: indexed answer disagrees with the oracle"));
        }
        if answered != "indexed" {
            self.finding(format!(
                "{label}: answered by {answered}, not indexed evidence"
            ));
        }
        if after_warm > BUDGET_BYTES {
            self.finding(format!(
                "{label}: peak {after_warm} bytes exceeds the budget"
            ));
        }
        let exact = self.exact(cassie, session, spec, &label);
        let record = json!({
            "suite": self.suite,
            "family": spec.family,
            "case": spec.case,
            "sql": spec.sql,
            "fixture": spec.fixture,
            "result_cache": { "enabled": false, "hits_cold": cold.counts.result_cache_hits,
                "hits_warm": warm.counts.result_cache_hits },
            "plan": { "explain": plan, "answered_by": answered, "fallback": fallback,
                "indexed_evidence": answered == "indexed" && indexed_matches },
            "returned_paths": cold.rows.len(),
            "oracle_rows": spec.expected.len(),
            "oracle": { "indexed_matches": indexed_matches, "exact": exact },
            "cold": cold.counts.to_json(),
            "warm": warm.counts.to_json(),
            "cold_minus_warm_graph_storage_reads":
                i128::from(cold.counts.graph_storage_reads())
                    - i128::from(warm.counts.graph_storage_reads()),
            "peak_query_memory_bytes": { "after_setup": after_setup, "after_cold": after_cold,
                "after_warm": after_warm, "budget": BUDGET_BYTES,
                "within_budget": after_warm <= BUDGET_BYTES },
        });
        println!("graph_evidence={record}");
        self.records.push(record);
        (cold.counts, warm.counts)
    }

    fn exact(
        &mut self,
        cassie: &Cassie,
        session: &CassieSession,
        spec: &Spec<'_>,
        label: &str,
    ) -> Json {
        if !spec.exact_switch {
            return json!({ "status": "not_applicable",
                "note": "no exact-read switch for this function; the oracle is the exact comparator" });
        }
        cassie.set_graph_exact_reads(true);
        let outcome = run(cassie, session, spec);
        cassie.set_graph_exact_reads(false);
        match outcome {
            Ok(exact) if exact.rows == spec.expected => {
                json!({ "status": "matched", "counts": exact.counts.to_json() })
            }
            Ok(exact) => {
                self.finding(format!("{label}: exact answer disagrees with the oracle"));
                json!({ "status": "mismatch", "counts": exact.counts.to_json() })
            }
            Err(state) => json!({ "status": "unavailable", "sqlstate": state,
                "note": "the exact full read did not complete within the query budget" }),
        }
    }

    /// Writes the records, then panics when any finding was raised so a bad run cannot pass
    /// as evidence.
    ///
    /// # Panics
    ///
    /// Panics when the file cannot be written or a finding was raised.
    pub fn finish(self) {
        let dir = std::env::var_os("CASSIE_BENCH_GRAPH_EVIDENCE_DIR")
            .map_or_else(|| PathBuf::from("target/graph-evidence"), PathBuf::from);
        std::fs::create_dir_all(&dir).expect("graph evidence directory");
        let document = json!({ "suite": self.suite, "records": self.records,
            "findings": self.findings });
        std::fs::write(
            dir.join(format!("{}.json", self.suite)),
            serde_json::to_vec_pretty(&document).expect("graph evidence json"),
        )
        .expect("write graph evidence");
        assert!(
            self.findings.is_empty(),
            "graph evidence raised findings: {:?}",
            self.findings
        );
    }
}
