//! Stable SQL-facing spellings for index options.

pub const ACCESS_METHOD_OPTION: &str = "__cassie_access_method";
pub const NULLS_NOT_DISTINCT_OPTION: &str = "__cassie_nulls_not_distinct";

/// Index option key for a per-key option such as `order` or `operator_class`.
#[must_use]
pub fn key_option_name(position: usize, option: &str) -> String {
    format!("__cassie_index_key_{position}_{option}")
}
