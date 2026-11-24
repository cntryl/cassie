//! Primitive output schemas for selected FROM-only catalog functions.
use crate::types::DataType;
use super::{is_options_table, is_pg_input_error_info};

pub(crate) fn catalog_table_columns(name: &str) -> Option<Vec<(String, DataType)>> {
    if super::is_loaded_modules(name) {
        return Some(super::loaded_modules_columns());
    }
    if super::object_identity::is_identify_object(name) {
        return Some(super::object_identity::identify_object_columns());
    }
    if let Some(columns) = super::graph_types::table_columns(name) {
        return Some(columns);
    }
    if super::graph_weighted::is_weighted_paths(name) {
        return Some(super::graph_weighted::columns(None));
    }
    if is_pg_input_error_info(name) {
        return Some(
            ["message", "detail", "hint", "sql_error_code"]
                .into_iter()
                .map(|column| (column.to_owned(), DataType::Text))
                .collect(),
        );
    }
    is_options_table(name).then(|| {
        ["option_name", "option_value"]
            .into_iter()
            .map(|name| (name.into(), DataType::Text))
            .collect()
    })
}
