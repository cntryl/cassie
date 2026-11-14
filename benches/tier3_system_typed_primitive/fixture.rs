//! Deterministic logical data and an independent checked-integer oracle.
use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;
use std::fmt::Write as _;
use std::sync::Arc;

pub const ROWS: usize = 100_000;
pub const QUERY_BYTES: usize = 2 * 1024 * 1024 * 1024;
pub const NATIVE_BYTES: usize = 2 * 1024 * 1024 * 1024;
pub const MEMTABLE_BYTES: usize = 512 * 1024 * 1024;
pub const FIXTURE: &str = "tier3_system_typed_primitive/100k";

#[derive(Clone, Copy)]
pub struct Case {
    pub name: &'static str,
    pub table: &'static str,
    pub not_false: bool,
    pub authority: &'static str,
    pub derived_index: bool,
}
pub const CASES: [Case; 6] = [
    Case {
        name: "row_true",
        table: "primitive_row",
        not_false: false,
        authority: "row",
        derived_index: false,
    },
    Case {
        name: "column_true",
        table: "primitive_column",
        not_false: false,
        authority: "column",
        derived_index: false,
    },
    Case {
        name: "indexed_true",
        table: "primitive_indexed",
        not_false: false,
        authority: "row",
        derived_index: true,
    },
    Case {
        name: "row_not_false",
        table: "primitive_row",
        not_false: true,
        authority: "row",
        derived_index: false,
    },
    Case {
        name: "column_not_false",
        table: "primitive_column",
        not_false: true,
        authority: "column",
        derived_index: false,
    },
    Case {
        name: "indexed_not_false",
        table: "primitive_indexed",
        not_false: true,
        authority: "row",
        derived_index: true,
    },
];
impl Case {
    pub fn sql(self) -> String {
        format!(
            "SELECT SUM(n),COUNT(n),COUNT(*) FROM {} WHERE b IS {} AND n>=0",
            self.table,
            if self.not_false { "NOT FALSE" } else { "TRUE" }
        )
    }
    pub fn expected(self) -> Vec<Vec<Value>> {
        // Independent exhaustive arithmetic oracle for the declared 100k formula,
        // checked separately from CSV generation (assembler's Python receipt).
        let (sum, count): (i128, i64) = if self.not_false {
            (54_043_195_543_578_022, 30_288)
        } else {
            (27_021_597_771_773_687, 15_121)
        };
        vec![vec![
            Value::Int64(i64::try_from(sum).expect("fixture SUM fits BIGINT")),
            Value::Int64(count),
            Value::Int64(count),
        ]]
    }
}
fn integer(id: usize) -> Option<i64> {
    if id.is_multiple_of(11) {
        None
    } else if id % 10_000 == 1 {
        Some(9_007_199_254_740_993)
    } else {
        Some(i64::try_from(id % 2_001).expect("small fixture integer") - 1_000)
    }
}
fn csv(start: usize, end: usize) -> String {
    let mut text = String::new();
    for id in start..end {
        let n = integer(id).map_or_else(String::new, |value| value.to_string());
        let b = match id % 3 {
            0 => "true",
            1 => "false",
            _ => "",
        };
        writeln!(text, "{n},{b}").expect("String write");
    }
    text
}
pub struct Db {
    pub cassie: Arc<Cassie>,
    pub session: CassieSession,
}
impl Db {
    pub fn seed() -> Self {
        let mut config = CassieRuntimeConfig::from_env().expect("benchmark config");
        config.storage_memory_budget_bytes = Some(NATIVE_BYTES);
        config.storage_memtable_size_bytes = Some(MEMTABLE_BYTES);
        config.limits.query_memory_budget_bytes = QUERY_BYTES;
        config.limits.query_timeout_ms = 0;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        config.limits.parallel_aggregation_workers = 1;
        config.limits.max_query_workers = 1;
        let cassie = Arc::new(Cassie::new_in_memory_with_config(config).expect("memory instance"));
        cassie.startup().expect("startup");
        let session = cassie.create_session("benchmark", None);
        let db = Self { cassie, session };
        for (table, clause) in [
            ("primitive_row", ""),
            ("primitive_column", " USING columnar"),
            ("primitive_indexed", ""),
        ] {
            db.query(&format!(
                "CREATE TABLE {table} (n BIGINT,b BOOLEAN){clause}"
            ));
            for start in (0..ROWS).step_by(5_000) {
                let end = (start + 5_000).min(ROWS);
                let statement = CopyStatement {
                    table: table.into(),
                    columns: vec![],
                    format: CopyFormat::Csv,
                    header: false,
                };
                let actual = db
                    .cassie
                    .copy_from_csv_stdin(&db.session, &statement, csv(start, end).as_bytes())
                    .expect("fixture COPY");
                assert_eq!(actual, end - start);
            }
        }
        db.query("CREATE INDEX primitive_cover ON primitive_indexed USING column (n,b) WITH (segment_size=1024)");
        db
    }
    pub fn query(&self, sql: &str) -> cassie::executor::QueryResult {
        self.cassie
            .execute_sql(&self.session, sql, vec![])
            .unwrap_or_else(|error| panic!("primitive SQL failed: {sql}: {error}"))
    }
    pub fn checked(&self, sql: &str, expected: &[Vec<Value>]) {
        let result = self.query(sql);
        assert_eq!(result.rows(), expected);
        assert_eq!(result.columns().len(), 3);
        for (column, oid) in result.columns().iter().zip([1700, 20, 20]) {
            assert_eq!(column.type_oid, oid);
        }
    }
}
