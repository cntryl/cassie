//! Independently recorded rows and packet literals for the selected SQL subset.
use std::sync::Arc;

use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;
use serde::Deserialize;

#[path = "relational_qualification/wire.rs"]
mod wire;
pub use wire::{cycle, run_wire};
#[path = "relational_qualification/assertions.rs"]
mod assertions;
pub use assertions::matches_row;

#[derive(Deserialize)]
pub struct Records {
    pub seeds: Vec<Seed>,
    pub primary: Vec<Case>,
    pub variants: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct Seed {
    pub table: String,
    pub create: String,
    pub insert: String,
    pub columns: Vec<String>,
    pub bind_rows: Vec<Vec<Parameter>>,
    pub expected_cells: Vec<Vec<ExpectedCell>>,
}

#[derive(Deserialize)]
pub struct Parameter {
    pub oid: i32,
    pub format: i16,
    pub hex: Option<String>,
    pub utf8: Option<String>,
}

#[derive(Deserialize)]
pub struct ExpectedCell {
    pub kind: String,
    pub value: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct Case {
    pub invariant: String,
    pub sql: String,
    pub expected_rows: Option<Vec<Vec<serde_json::Value>>>,
    pub ordered: bool,
    pub columns: Option<Vec<Column>>,
    pub error: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct Column {
    pub name: String,
    pub oid: i32,
    pub typlen: i16,
    pub typmod: i32,
}

pub fn records() -> Records {
    serde_json::from_str(include_str!("relational_qualification/records.json"))
        .expect("literal qualification records")
}

pub struct Fixture {
    pub session: CassieSession,
    pub cassie: Arc<Cassie>,
    _directory: Directory,
}

struct Directory(std::path::PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("qualification cleanup failed during unwind: {error}");
            } else {
                panic!("qualification cleanup failed: {error}");
            }
        }
    }
}

pub fn fixture() -> Fixture {
    fixture_with_vectorized_joins(false)
}

pub fn fixture_with_vectorized_joins(enabled: bool) -> Fixture {
    let directory = Directory(std::env::temp_dir().join(format!(
        "cassie-relational-qualification-{}",
        uuid::Uuid::new_v4()
    )));
    std::fs::create_dir(&directory.0).expect("create private directory");
    let mut config = CassieRuntimeConfig::default();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 64 * 1024 * 1024;
    config.limits.vectorized_joins_enabled = enabled;
    let cassie = Arc::new(
        Cassie::new_with_data_dir_and_config(&directory.0, config).expect("qualification engine"),
    );
    cassie.startup().expect("startup before seed");
    let session = cassie.create_session("tester", None);
    let records = records();
    cassie
        .execute_sql(&session, "CREATE SCHEMA triage", vec![])
        .expect("qualified schema");
    for seed in &records.seeds {
        cassie
            .execute_sql(&session, &seed.create, vec![])
            .expect("seed table");
    }
    let fixture = Fixture {
        session,
        cassie,
        _directory: directory,
    };
    let mut cycles = Vec::new();
    let mut expected_oids = Vec::new();
    for seed in &records.seeds {
        for row in &seed.bind_rows {
            cycles.push(cycle(&seed.insert, row, 0));
            expected_oids.push(row.iter().map(|param| param.oid).collect::<Vec<_>>());
        }
    }
    for (frames, oids) in run_wire(&fixture, cycles).iter().zip(expected_oids) {
        assert_eq!(crate::support_pgwire::error_code(frames), None);
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("seed parameter description");
        assert_eq!(
            crate::support_pgwire::parse_parameter_description(&description.1),
            oids
        );
    }
    fixture
}

pub fn assert_seed_carriers(fixture: &Fixture) {
    for seed in records().seeds {
        let sql = format!("SELECT {} FROM {}", seed.columns.join(","), seed.table);
        let result = fixture
            .cassie
            .execute_sql(&fixture.session, &sql, vec![])
            .expect("seed readback");
        assert_eq!(
            result.rows.len(),
            seed.expected_cells.len(),
            "{}",
            seed.table
        );
        let mut unmatched = result.rows;
        for expected in seed.expected_cells {
            let index = unmatched
                .iter()
                .position(|row| {
                    row.len() == expected.len()
                        && row.iter().zip(&expected).all(|(actual, cell)| {
                            match (actual, cell.kind.as_str()) {
                                (Value::Null, "null") => true,
                                (Value::Int64(value), "integer") => {
                                    value.to_string()
                                        == cell.value.as_ref().unwrap().as_str().unwrap()
                                }
                                (Value::Float64(value), "float_bits") => {
                                    format!("{:016x}", value.to_bits())
                                        == cell.value.as_ref().unwrap().as_str().unwrap()
                                }
                                (Value::String(value), "text") => {
                                    Some(value.as_str())
                                        == cell.value.as_ref().and_then(serde_json::Value::as_str)
                                }
                                (Value::Json(value), "array") => Some(value) == cell.value.as_ref(),
                                _ => false,
                            }
                        })
                })
                .unwrap_or_else(|| panic!("{}: missing literal carrier row", seed.table));
            unmatched.remove(index);
        }
        assert_eq!(unmatched, Vec::<Vec<Value>>::new());
    }
}
