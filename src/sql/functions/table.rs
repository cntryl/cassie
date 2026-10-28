//! Primitive output schemas for selected FROM-only catalog functions.
use crate::types::DataType;
pub(crate) fn is_options_table(name: &str) -> bool {
    name.eq_ignore_ascii_case("pg_options_to_table")
        || name.eq_ignore_ascii_case("pg_catalog.pg_options_to_table")
}
pub(crate) fn is_catalog_table(name: &str) -> bool {
    super::is_loaded_modules(name) || is_options_table(name) || is_pg_input_error_info(name)
}

pub(crate) fn is_pg_input_error_info(name: &str) -> bool {
    name.eq_ignore_ascii_case("pg_input_error_info")
        || name.eq_ignore_ascii_case("pg_catalog.pg_input_error_info")
}
pub(crate) fn catalog_table_columns(name: &str) -> Option<Vec<(String, DataType)>> {
    if super::is_loaded_modules(name) {
        return Some(super::loaded_modules_columns());
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
