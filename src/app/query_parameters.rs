//! Canonicalizes bound string parameters by their inferred SQL type.
//!
//! Inline `UUID`, `BYTEA`, `DATE`, `TIME` and `TIMESTAMP` literals are
//! canonicalized against their column type by the binder and on write. A
//! bound `Value::String` parameter must match the same rows, so each one
//! whose type the statement implies is rewritten to the same canonical text.

use crate::catalog::Catalog;
use crate::sql::ast::ParsedStatement;
use crate::types::{DataType, Value};

pub(super) fn canonicalize_string_parameters(
    parsed: &ParsedStatement,
    catalog: &Catalog,
    params: &mut [Value],
) {
    if !params.iter().any(|value| matches!(value, Value::String(_))) {
        return;
    }
    let oids = crate::sql::parameter_type_oids_with_catalog(parsed, &[], catalog);
    for (param, oid) in params.iter_mut().zip(oids) {
        let Value::String(text) = param else {
            continue;
        };
        let canonical = match crate::sql::binder::parameter_data_type_for_oid(oid) {
            Some(DataType::Uuid) => uuid::Uuid::parse_str(text)
                .ok()
                .map(|uuid| uuid.to_string()),
            Some(DataType::Bytea) => crate::midge::row_blob::canonical_bytea_text(text).ok(),
            Some(DataType::Date) => crate::types::temporal::canonical_date(text).ok(),
            Some(DataType::Time) => crate::types::temporal::canonical_time(text).ok(),
            Some(DataType::Timestamp) => crate::types::temporal::canonical_timestamp(text).ok(),
            _ => None,
        };
        if let Some(canonical) = canonical {
            *text = canonical;
        }
    }
}
