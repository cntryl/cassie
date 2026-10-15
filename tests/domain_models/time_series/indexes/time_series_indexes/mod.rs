#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::IndexKind;
use cassie::sql::ast::QueryStatement;
use cassie::types::Value;
use serde_json::json;

use super::support_sql as support;
use super::support_time_series_evidence::TimeSeriesWidthEvidence;
use support::*;

include!("semantics.rs");

include!("errors.rs");

include!("restart.rs");

include!("parameters.rs");

fn same_second_event_amounts(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    sql: &str,
) -> Vec<i64> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect(sql)
        .rows()
        .iter()
        .map(|row| match row.first() {
            Some(Value::Int64(amount)) => *amount,
            other => panic!("expected integer amount, got {other:?}"),
        })
        .collect()
}
