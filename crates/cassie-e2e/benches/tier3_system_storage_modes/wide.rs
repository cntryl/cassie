//! Paired fixture seeding, including the wide fact pair whose extra payload
//! column is never selected by the benchmark queries.

use std::fmt::Write as _;

use cassie::sql::ast::{CopyFormat, CopyStatement};

use super::{data, Db};

const COPY_BATCH_ROWS: usize = 5_000;
const WIDE_COPY_BATCH_ROWS: usize = 1_000;
const PAYLOAD_REPEATS: usize = 64;

pub const WIDE_PAIR: [(&str, data::Storage); 2] = [
    ("f_row_wide", data::Storage::Row),
    ("f_col_wide", data::Storage::Column),
];

/// Same eight columns as the narrow fact plus `payload TEXT` (about 1 KiB).
pub fn create_wide_sql(table: &str, storage: data::Storage) -> String {
    let base = data::create_fact_sql(table, data::Storage::Row);
    let base = base.strip_suffix(')').expect("fact DDL ends with a paren");
    let clause = match storage {
        data::Storage::Row => "",
        data::Storage::Column => " USING columnar",
    };
    format!("{base}, payload TEXT){clause}")
}

/// A deterministic 1 KiB payload without CSV metacharacters.
pub fn payload(i: usize) -> String {
    format!(
        "{:016x}",
        u64::try_from(i)
            .expect("row index")
            .wrapping_mul(0x9E37_79B1)
    )
    .repeat(PAYLOAD_REPEATS)
}

pub fn wide_csv(range: std::ops::Range<usize>) -> String {
    let mut out = String::new();
    for i in range {
        let line = data::fact_csv(i..i + 1);
        let _ = writeln!(out, "{},{}", line.trim_end(), payload(i));
    }
    out
}

fn copy(db: &Db, table: &str, csv: &str, expected: usize) {
    let statement = CopyStatement {
        table: table.to_owned(),
        columns: vec![],
        format: CopyFormat::Csv,
        header: false,
    };
    let actual = db
        .cassie
        .copy_from_csv_stdin(&db.session, &statement, csv.as_bytes())
        .unwrap_or_else(|error| panic!("fixture COPY into {table} failed: {error}"));
    assert_eq!(actual, expected, "fixture COPY row count for {table}");
}

fn load(
    db: &Db,
    table: &str,
    rows: usize,
    batch: usize,
    csv: fn(std::ops::Range<usize>) -> String,
) {
    for start in (0..rows).step_by(batch) {
        let end = (start + batch).min(rows);
        copy(db, table, &csv(start..end), end - start);
    }
}

/// Seeds `f_row`/`f_col`, `d_row`/`d_col` and the wide pair. Returns the
/// elapsed setup time in nanoseconds for the report.
pub fn seed(db: &Db, fact_rows: usize, wide_rows: usize) -> u128 {
    let started = std::time::Instant::now();
    let dims = data::dim_csv();
    for (fact, dim, mode) in [
        ("f_row", "d_row", data::Storage::Row),
        ("f_col", "d_col", data::Storage::Column),
    ] {
        db.exec(&data::create_fact_sql(fact, mode));
        load(db, fact, fact_rows, COPY_BATCH_ROWS, data::fact_csv);
        db.exec(&data::create_dim_sql(dim, mode));
        copy(db, dim, &dims, data::DIM_ROWS);
    }
    for (table, mode) in WIDE_PAIR {
        db.exec(&create_wide_sql(table, mode));
        load(db, table, wide_rows, WIDE_COPY_BATCH_ROWS, wide_csv);
    }
    started.elapsed().as_nanos()
}
