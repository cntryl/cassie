//! Retain explicit CTE mode while leaving parenthesized-body parsing shared.
use crate::sql::ast::CteMaterialization;

pub(super) fn parse(raw: &str) -> (CteMaterialization, &str) {
    for (keyword, mode) in [
        ("not materialized", CteMaterialization::NotMaterialized),
        ("materialized", CteMaterialization::Materialized),
    ] {
        if let Some(end) = super::lexical::pattern_end(raw, 0, keyword) {
            return (mode, super::lexical::trim_separators(&raw[end..]));
        }
    }
    (CteMaterialization::Default, raw)
}
