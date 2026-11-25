//! Deterministic fixture data and SQL text for the storage-mode benchmark.

use std::fmt::Write as _;

pub const DIM_ROWS: usize = 1_000;

const FACT_COLUMNS: &str = "id, grp, cat, amount, qty, flag, note, dim_id";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Storage {
    Row,
    Column,
}

impl Storage {
    const fn clause(self) -> &'static str {
        match self {
            Self::Row => "",
            Self::Column => " USING columnar",
        }
    }
}

pub fn create_fact_sql(table: &str, storage: Storage) -> String {
    format!(
        "CREATE TABLE {table} (id BIGINT PRIMARY KEY, grp INTEGER NOT NULL, cat TEXT NOT NULL, \
         amount DOUBLE PRECISION, qty INTEGER NOT NULL, flag BOOLEAN NOT NULL, note TEXT, \
         dim_id INTEGER NOT NULL){}",
        storage.clause()
    )
}

pub fn create_dim_sql(table: &str, storage: Storage) -> String {
    format!(
        "CREATE TABLE {table} (dim_id INTEGER PRIMARY KEY, region TEXT NOT NULL, \
         weight INTEGER NOT NULL){}",
        storage.clause()
    )
}

fn small(i: usize, modulus: usize) -> u32 {
    u32::try_from(i % modulus).expect("modulus fits u32")
}

/// Amounts are multiples of 0.25 so every sum is exact regardless of scan order.
fn amount(i: usize) -> Option<f64> {
    (!i.is_multiple_of(7)).then(|| f64::from(small(i, 1_000)) * 0.25)
}

fn note(i: usize) -> Option<String> {
    (!i.is_multiple_of(5)).then(|| format!("note-{i}"))
}

pub fn fact_csv(range: std::ops::Range<usize>) -> String {
    let mut out = String::new();
    for i in range {
        let amount = amount(i).map_or_else(String::new, |value| value.to_string());
        let note = note(i).unwrap_or_default();
        let _ = writeln!(
            out,
            "{i},{},cat{},{amount},{},{},{note},{}",
            small(i, 100),
            small(i, 8),
            small(i * 7, 1_000),
            i.is_multiple_of(3),
            small(i, DIM_ROWS),
        );
    }
    out
}

pub fn dim_csv() -> String {
    let mut out = String::new();
    for d in 0..DIM_ROWS {
        let _ = writeln!(out, "{d},region{},{}", small(d, 10), small(d, 50));
    }
    out
}

pub fn full_scan_sql(table: &str) -> String {
    format!("SELECT {FACT_COLUMNS} FROM {table}")
}

pub fn narrow_sql(table: &str) -> String {
    format!("SELECT grp, qty FROM {table}")
}

pub fn filtered_agg_sql(table: &str) -> String {
    format!(
        "SELECT cat, COUNT(*) AS n, SUM(qty) AS q, SUM(amount) AS a FROM {table} \
         WHERE grp < 50 GROUP BY cat"
    )
}

pub fn join_agg_sql(fact: &str, dim: &str) -> String {
    format!(
        "SELECT d.region, COUNT(*) AS n, SUM(f.qty) AS q, SUM(d.weight) AS w FROM {fact} f \
         JOIN {dim} d ON f.dim_id = d.dim_id WHERE f.grp < 50 GROUP BY d.region"
    )
}
