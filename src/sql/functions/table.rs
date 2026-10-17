//! Primitive output schemas for selected FROM-only catalog functions.
use crate::types::DataType;
pub(crate) fn is_options_table(name: &str) -> bool {
    name.eq_ignore_ascii_case("pg_options_to_table")
        || name.eq_ignore_ascii_case("pg_catalog.pg_options_to_table")
}
pub(crate) fn is_catalog_table(name: &str) -> bool {
    super::is_loaded_modules(name) || is_options_table(name)
}
pub(crate) fn catalog_table_columns(name: &str) -> Option<Vec<(String, DataType)>> {
    if super::is_loaded_modules(name) {
        return Some(super::loaded_modules_columns());
    }
    is_options_table(name).then(|| {
        ["option_name", "option_value"]
            .into_iter()
            .map(|name| (name.into(), DataType::Text))
            .collect()
    })
}
