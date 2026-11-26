//! Paired fixture seeding, including the wide fact pair whose extra payload
//! column is never selected by the benchmark queries.

use std::fmt::Write as _;

use cassie::sql::ast::{CopyFormat, CopyStatement};

use super::{data, profiles::Fixture, Db};

const COPY_BATCH_ROWS: usize = 5_000;
const WIDE_COPY_BATCH_ROWS: usize = 1_000;
pub(super) const LOCAL_COPY_BATCH_ROWS: usize = 250;
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

fn copy(db: &Db, table: &str, csv: &str, start: usize, end: usize) -> Result<(), String> {
    let statement = CopyStatement {
        table: table.to_owned(),
        columns: vec![],
        format: CopyFormat::Csv,
        header: false,
    };
    let actual = db
        .cassie
        .copy_from_csv_stdin(&db.session, &statement, csv.as_bytes())
        .map_err(|error| {
            format!(
                "fixture COPY into {table} at [{start}, {end}) after {start} acknowledged rows: {} ({error})",
                error.sql_state()
            )
        })?;
    if actual != end - start {
        return Err(format!(
            "fixture COPY into {table} at [{start}, {end}) after {start} acknowledged rows returned {actual} rows"
        ));
    }
    Ok(())
}

fn load(
    db: &Db,
    table: &str,
    rows: usize,
    batch: usize,
    csv: fn(std::ops::Range<usize>) -> String,
) -> Result<(), String> {
    for start in (0..rows).step_by(batch) {
        let end = (start + batch).min(rows);
        copy(db, table, &csv(start..end), start, end)?;
    }
    println!("storage_modes_fixture table={table} acknowledged_rows={rows}");
    Ok(())
}

/// Seeds only the declared fixtures and checks every COPY acknowledgement
/// before any measurement begins.
pub fn seed(
    db: &Db,
    fact_rows: usize,
    wide_rows: usize,
    fixture: Fixture,
    local: bool,
) -> Result<u128, String> {
    let started = std::time::Instant::now();
    let (narrow_batch, wide_batch) = if local {
        (LOCAL_COPY_BATCH_ROWS, LOCAL_COPY_BATCH_ROWS)
    } else {
        (COPY_BATCH_ROWS, WIDE_COPY_BATCH_ROWS)
    };
    if fixture.includes_narrow() {
        let dims = data::dim_csv();
        for (fact, dim, mode) in [
            ("f_row", "d_row", data::Storage::Row),
            ("f_col", "d_col", data::Storage::Column),
        ] {
            db.try_exec(&data::create_fact_sql(fact, mode))?;
            load(db, fact, fact_rows, narrow_batch, data::fact_csv)?;
            db.try_exec(&data::create_dim_sql(dim, mode))?;
            copy(db, dim, &dims, 0, data::DIM_ROWS)?;
            println!(
                "storage_modes_fixture table={dim} acknowledged_rows={}",
                data::DIM_ROWS
            );
        }
    }
    if fixture.includes_wide() {
        for (table, mode) in WIDE_PAIR {
            db.try_exec(&create_wide_sql(table, mode))?;
            load(db, table, wide_rows, wide_batch, wide_csv)?;
        }
    }
    Ok(started.elapsed().as_nanos())
}
