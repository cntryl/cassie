use crate::catalog::{
    relation_belongs_to_database, relation_schema_name, Catalog, SequenceMeta, local_name,
};
use crate::types::{DataType, Value};

use super::VirtualRow;

pub(super) fn information_schema_sequences_schema() -> Vec<(String, DataType)> {
    vec![
        text("sequence_catalog"),
        text("sequence_schema"),
        text("sequence_name"),
        text("data_type"),
        int("numeric_precision"),
        int("numeric_precision_radix"),
        int("numeric_scale"),
        text("start_value"),
        text("minimum_value"),
        text("maximum_value"),
        text("increment"),
        text("cycle_option"),
    ]
}

pub(super) fn information_schema_sequences(
    catalog: &Catalog,
    current_database: Option<&str>,
) -> Vec<VirtualRow> {
    catalog
        .list_sequences()
        .into_iter()
        .filter(|sequence| {
            current_database
                .is_none_or(|database| relation_belongs_to_database(&sequence.name, database))
        })
        .map(sequence_row)
        .collect()
}

fn sequence_row(sequence: SequenceMeta) -> VirtualRow {
    vec![
        string(
            "sequence_catalog",
            crate::catalog::relation_database_name(&sequence.name).unwrap_or_else(|| {
                "cassie".to_string()
            }),
        ),
        string("sequence_schema", relation_schema_name(&sequence.name)),
        string("sequence_name", local_name(&sequence.name)),
        string("data_type", sequence_data_type(&sequence.data_type)),
        int_value("numeric_precision", numeric_precision(&sequence.data_type)),
        int_value("numeric_precision_radix", 2),
        int_value("numeric_scale", 0),
        string("start_value", sequence.start_value.to_string()),
        string("minimum_value", "1"),
        string(
            "maximum_value",
            maximum_value(&sequence.data_type).to_string(),
        ),
        string("increment", sequence.increment_by.to_string()),
        string("cycle_option", "NO"),
    ]
}

fn sequence_data_type(data_type: &DataType) -> String {
    match data_type {
        DataType::SmallInt => "smallint".to_string(),
        DataType::Int => "integer".to_string(),
        DataType::BigInt => "bigint".to_string(),
        _ => data_type.type_name(),
    }
}

fn numeric_precision(data_type: &DataType) -> i64 {
    match data_type {
        DataType::SmallInt => 16,
        DataType::Int => 32,
        _ => 64,
    }
}

fn maximum_value(data_type: &DataType) -> i64 {
    match data_type {
        DataType::SmallInt => i16::MAX.into(),
        DataType::Int => i32::MAX.into(),
        _ => i64::MAX,
    }
}

fn text(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Text)
}

fn int(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Int)
}

fn string(name: &str, value: impl Into<String>) -> (String, Value) {
    (name.to_string(), Value::String(value.into()))
}

fn int_value(name: &str, value: i64) -> (String, Value) {
    (name.to_string(), Value::Int64(value))
}
