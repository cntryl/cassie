//! `pg_cursors` and `pg_prepared_statements`: the current session's open
//! cursors and its SQL and protocol prepared statements.
//!
//! `parameter_types` and `result_types` are shown as `text` in PostgreSQL's
//! `{type,...}` array spelling rather than as `regtype[]`.

use super::VirtualRow;
use crate::app::CassieSession;
use crate::types::extended::ExtendedType;
use crate::types::typed::TypedValue;
use crate::types::{DataType, Value};

const CURSORS: &str = "pg_catalog.pg_cursors";
const PREPARED: &str = "pg_catalog.pg_prepared_statements";

fn timestamptz() -> DataType {
    DataType::Extended(ExtendedType::TimestampTz { precision: None })
}

pub(super) fn schema(name: &str) -> Option<Vec<(String, DataType)>> {
    let columns = match name {
        CURSORS => vec![
            ("name", DataType::Text),
            ("statement", DataType::Text),
            ("is_holdable", DataType::Boolean),
            ("is_binary", DataType::Boolean),
            ("is_scrollable", DataType::Boolean),
            ("creation_time", timestamptz()),
        ],
        PREPARED => vec![
            ("name", DataType::Text),
            ("statement", DataType::Text),
            ("prepare_time", timestamptz()),
            ("parameter_types", DataType::Text),
            ("result_types", DataType::Text),
            ("from_sql", DataType::Boolean),
            ("generic_plans", DataType::BigInt),
            ("custom_plans", DataType::BigInt),
        ],
        _ => return None,
    };
    Some(
        columns
            .into_iter()
            .map(|(name, data_type)| (name.to_string(), data_type))
            .collect(),
    )
}

pub(super) fn rows(name: &str, session: Option<&CassieSession>) -> Option<Vec<VirtualRow>> {
    if !matches!(name, CURSORS | PREPARED) {
        return None;
    }
    let Some(session) = session else {
        return Some(Vec::new());
    };
    Some(session.with_session_objects(|objects| {
        if name == CURSORS {
            objects
                .cursors()
                .map(|(name, cursor)| {
                    vec![
                        text("name", name),
                        text("statement", &cursor.statement_text),
                        boolean("is_holdable", cursor.options.hold),
                        boolean("is_binary", cursor.options.binary),
                        boolean("is_scrollable", cursor.options.scroll),
                        timestamp("creation_time", cursor.creation_time),
                    ]
                })
                .collect()
        } else {
            objects
                .prepared_statements()
                .map(|(name, prepared)| {
                    let parameters = prepared
                        .parameter_types
                        .iter()
                        .map(|name| crate::app::prepared_parameter_type_display(name))
                        .collect::<Vec<_>>();
                    vec![
                        text("name", name),
                        text("statement", &prepared.statement_text),
                        timestamp("prepare_time", prepared.prepare_time),
                        text("parameter_types", &array_text(&parameters)),
                        text("result_types", &array_text(&prepared.result_types)),
                        boolean("from_sql", prepared.wire_id.is_none()),
                        ("generic_plans".to_string(), Value::Int64(0)),
                        ("custom_plans".to_string(), Value::Int64(0)),
                    ]
                })
                .collect()
        }
    }))
}

fn array_text(items: &[String]) -> String {
    format!("{{{}}}", items.join(","))
}

fn text(name: &str, value: &str) -> (String, Value) {
    (name.to_string(), Value::String(value.to_string()))
}

fn boolean(name: &str, value: bool) -> (String, Value) {
    (name.to_string(), Value::Bool(value))
}

fn timestamp(name: &str, micros: i64) -> (String, Value) {
    (
        name.to_string(),
        Value::Typed(TypedValue::TimestampTz(micros)),
    )
}
