//! The paired query set. Each query names its heap arm and its column arm SQL
//! plus the independent oracle used to verify both before timing.

use cassie::types::Value;

use super::{data, expected, wide};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Subject to the advantage gate.
    Primary,
    /// Recorded in the report only.
    Reported,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Reported => "reported",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Oracle {
    Narrow,
    Aggregate,
    Join,
    FullScan,
    /// Rows with `lo <= id < hi`, projecting `(id, qty)`.
    Selective {
        lo: usize,
        hi: usize,
    },
}

#[derive(Clone, Debug)]
pub struct Query {
    pub name: String,
    pub role: Role,
    pub heap_sql: String,
    pub col_sql: String,
    pub oracle: Oracle,
    /// Rows in the fact table the query reads.
    pub rows: usize,
    /// Single-collection queries have one meaningful `storage_mode` token.
    pub single_table: bool,
}

impl Query {
    /// The independent expectation, or `None` when materializing it would cost
    /// more than the measurement is worth (unfiltered scans beyond 100k rows).
    pub fn oracle_rows(&self) -> Option<Vec<Vec<Value>>> {
        match self.oracle {
            Oracle::Narrow => Some(expected::rows("narrow", self.rows)),
            Oracle::Aggregate => Some(expected::rows("aggregate", self.rows)),
            Oracle::Join => Some(expected::rows("join", self.rows)),
            Oracle::FullScan => {
                (self.rows <= 100_000).then(|| expected::rows("full_scan", self.rows))
            }
            Oracle::Selective { lo, hi } => Some(
                (lo..hi)
                    .map(|id| {
                        let row = expected::fact_row(id);
                        vec![row[0].clone(), row[4].clone()]
                    })
                    .collect(),
            ),
        }
    }
}

/// An aligned selective range: 1% of the rows starting at the midpoint, so
/// the predicate matches contiguous primary-key order and can prune once
/// segment pruning lands.
fn selective_range(rows: usize) -> (usize, usize) {
    let lo = rows / 2;
    (lo, lo + (rows / 100).max(1))
}

fn selective_sql(table: &str, lo: usize, hi: usize) -> String {
    format!("SELECT id, qty FROM {table} WHERE id >= {lo} AND id < {hi}")
}

type SqlBuilder = fn(&str) -> String;

pub fn all(fact_rows: usize, wide_rows: usize) -> Vec<Query> {
    let mut queries = Vec::new();
    let (lo, hi) = selective_range(fact_rows);
    let shapes: [(&str, Role, Oracle, SqlBuilder); 2] = [
        (
            "narrow_projection",
            Role::Primary,
            Oracle::Narrow,
            data::narrow_sql,
        ),
        (
            "filtered_aggregate",
            Role::Primary,
            Oracle::Aggregate,
            data::filtered_agg_sql,
        ),
    ];
    for (name, role, oracle, sql) in shapes {
        queries.push(Query {
            name: name.to_owned(),
            role,
            heap_sql: sql("f_row"),
            col_sql: sql("f_col"),
            oracle,
            rows: fact_rows,
            single_table: true,
        });
    }
    queries.push(Query {
        name: "full_scan".to_owned(),
        role: Role::Reported,
        heap_sql: data::full_scan_sql("f_row"),
        col_sql: data::full_scan_sql("f_col"),
        oracle: Oracle::FullScan,
        rows: fact_rows,
        single_table: true,
    });
    queries.push(Query {
        name: "aligned_selective".to_owned(),
        role: Role::Reported,
        heap_sql: selective_sql("f_row", lo, hi),
        col_sql: selective_sql("f_col", lo, hi),
        oracle: Oracle::Selective { lo, hi },
        rows: fact_rows,
        single_table: true,
    });
    for (name, fact, dim) in [
        ("join_row_fact_row_dim", "f_row", "d_row"),
        ("join_col_fact_col_dim", "f_col", "d_col"),
        ("join_row_fact_col_dim", "f_row", "d_col"),
        ("join_col_fact_row_dim", "f_col", "d_row"),
    ] {
        queries.push(Query {
            name: name.to_owned(),
            role: Role::Reported,
            heap_sql: data::join_agg_sql("f_row", "d_row"),
            col_sql: data::join_agg_sql(fact, dim),
            oracle: Oracle::Join,
            rows: fact_rows,
            single_table: false,
        });
    }
    let (wide_lo, wide_hi) = selective_range(wide_rows);
    let [(heap_wide, _), (col_wide, _)] = wide::WIDE_PAIR;
    queries.push(Query {
        name: "wide_narrow_projection".to_owned(),
        role: Role::Reported,
        heap_sql: data::narrow_sql(heap_wide),
        col_sql: data::narrow_sql(col_wide),
        oracle: Oracle::Narrow,
        rows: wide_rows,
        single_table: true,
    });
    queries.push(Query {
        name: "wide_filtered_aggregate".to_owned(),
        role: Role::Reported,
        heap_sql: data::filtered_agg_sql(heap_wide),
        col_sql: data::filtered_agg_sql(col_wide),
        oracle: Oracle::Aggregate,
        rows: wide_rows,
        single_table: true,
    });
    queries.push(Query {
        name: "wide_aligned_selective".to_owned(),
        role: Role::Reported,
        heap_sql: selective_sql(heap_wide, wide_lo, wide_hi),
        col_sql: selective_sql(col_wide, wide_lo, wide_hi),
        oracle: Oracle::Selective {
            lo: wide_lo,
            hi: wide_hi,
        },
        rows: wide_rows,
        single_table: true,
    });
    queries
}
