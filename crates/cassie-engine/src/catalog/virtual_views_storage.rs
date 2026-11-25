use super::VirtualRow;
use crate::catalog::{local_name, relation_belongs_to_database, relation_schema_name, Catalog};
use crate::types::{DataType, Value};

pub(super) fn schema() -> Vec<(String, DataType)> {
    vec![
        text("schemaname"),
        text("tablename"),
        text("storage_mode"),
        int("storage_version"),
    ]
}

pub(super) fn rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_collections_canonical()
        .into_iter()
        .filter_map(|collection| {
            if current_database
                .is_some_and(|database| !relation_belongs_to_database(&collection.name, database))
            {
                return None;
            }
            let metadata = catalog.get_collection(&collection.name)?;
            let storage_mode = catalog.collection_storage_mode(&collection.name)?;
            Some(vec![
                string("schemaname", relation_schema_name(&collection.name)),
                string("tablename", local_name(&collection.name)),
                string("storage_mode", storage_mode.as_str()),
                int_value("storage_version", i64::from(metadata.storage_version)),
            ])
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| {
        row.iter()
            .map(|(name, value)| format!("{name}:{value:?}"))
            .collect::<Vec<_>>()
            .join("|")
    });
    rows
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
