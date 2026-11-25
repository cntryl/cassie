//! Source identities for selected explicit CAST families.
use super::{Catalog, DataType, Expr, ResultTypes};

pub(super) fn cast_source_type(
    expr: &Expr,
    target: &DataType,
    types: &ResultTypes,
    catalog: &Catalog,
) -> Option<DataType> {
    if target.is_json() {
        return types.expression_type(expr).filter(DataType::is_json);
    }
    if let DataType::Array(element) = target {
        return types.expression_type(expr).filter(|source| {
            matches!(source, DataType::Array(_))
                && (source == target
                    || matches!(
                        element.as_ref(),
                        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
                    ))
        });
    }
    if *target == DataType::Date {
        return types
            .expression_type(expr)
            .filter(|source| *source == DataType::Timestamp);
    }
    if *target == DataType::Bytea || crate::types::IntegerWidth::from_data_type(target).is_some() {
        return types.expression_type(expr).filter(|source| {
            (*target == DataType::Bytea
                && crate::types::IntegerWidth::from_data_type(source).is_some())
                || (*source == DataType::Bytea
                    && crate::types::IntegerWidth::from_data_type(target).is_some())
        });
    }
    if !matches!(
        target,
        DataType::Text | DataType::Varchar { .. } | DataType::Char { .. }
    ) {
        return None;
    }
    if matches!(target, DataType::Text | DataType::Varchar { .. }) {
        if let Some(source @ DataType::Array(_)) = types.expression_type(expr) {
            return Some(source);
        }
    }
    // These selected output conversions need source identity. Unresolved
    // NUMERIC carriers must not be relabeled as proven FLOAT8, and no heap
    // type descriptor is cloned just to annotate an unrelated cast.
    types.expression_type(expr).filter(|kind| {
        matches!(
            kind,
            DataType::Time
                | DataType::Timestamp
                | DataType::Char { .. }
                | DataType::ObjectId(_)
                | DataType::Jsonb
        ) || (*kind == DataType::Float && types.proven_float8(expr, catalog))
    })
}
