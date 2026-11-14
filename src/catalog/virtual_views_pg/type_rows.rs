//! `pg_type` rows for built-in types and their arrays, and the per-type
//! columns (`typlen`, `typbyval`, `typelem`, `typarray`) every row shares.

use super::{bool_value, int_value, namespace_oid, string, type_identity, VirtualRow};
use crate::catalog::Catalog;
use crate::types::DataType;

pub(super) fn builtin_type_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    let namespace = "pg_catalog";
    let types = crate::types::builtin::BUILTIN_TYPES;
    let mut rows: Vec<_> = types
        .iter()
        .map(|metadata| type_row(catalog, &metadata.kind.data_type(), namespace))
        .collect();
    rows.extend(
        types
            .iter()
            .filter(|metadata| metadata.kind != crate::types::builtin::BuiltinKind::Unknown)
            .map(|metadata| {
                type_row(
                    catalog,
                    &DataType::Array(Box::new(metadata.kind.data_type())),
                    namespace,
                )
            }),
    );

    rows
}

fn type_row(catalog: &Catalog, data_type: &DataType, namespace: &str) -> VirtualRow {
    type_row_with_oid(catalog, data_type, namespace, data_type.type_oid())
}

pub(super) fn type_row_with_oid(
    catalog: &Catalog,
    data_type: &DataType,
    namespace: &str,
    type_oid: i64,
) -> VirtualRow {
    let builtin_type = u32::try_from(type_oid)
        .ok()
        .and_then(|oid| crate::catalog::builtin_identity::builtin_identities().type_by_oid(oid));
    let typname =
        builtin_type.map_or_else(|| data_type.type_name(), |metadata| metadata.name.clone());
    vec![
        int_value("oid", type_oid),
        string("typname", typname),
        int_value("typnamespace", namespace_oid(catalog, namespace)),
        int_value("typlen", i64::from(data_type.typlen())),
        bool_value("typbyval", is_type_passed_by_value(data_type)),
        string("typtype", type_identity::kind(data_type, builtin_type)),
        string(
            "typcategory",
            type_identity::category(data_type, builtin_type),
        ),
        int_value(
            "typelem",
            builtin_type.map_or_else(
                || element_type_oid(data_type),
                |metadata| i64::from(metadata.element),
            ),
        ),
        int_value(
            "typarray",
            builtin_type.map_or_else(
                || array_type_oid(data_type),
                |metadata| i64::from(metadata.array),
            ),
        ),
        int_value("typbasetype", 0),
        bool_value("typnotnull", false),
        int_value("typtypmod", -1),
        int_value("typrelid", 0),
    ]
}

fn is_type_passed_by_value(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Boolean
            | DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Date
            | DataType::Time
            | DataType::Timestamp
            | DataType::ObjectId(_)
            | DataType::Extended(
                crate::types::extended::ExtendedType::Real
                    | crate::types::extended::ExtendedType::PgLsn
                    | crate::types::extended::ExtendedType::Date
                    | crate::types::extended::ExtendedType::Time { .. }
                    | crate::types::extended::ExtendedType::Timestamp { .. }
                    | crate::types::extended::ExtendedType::TimestampTz { .. }
            )
    )
}

fn element_type_oid(data_type: &DataType) -> i64 {
    match data_type {
        DataType::Array(inner) => inner.type_oid(),
        _ => 0,
    }
}

fn array_type_oid(data_type: &DataType) -> i64 {
    if matches!(data_type, DataType::Array(_)) {
        return 0;
    }

    u32::try_from(data_type.type_oid())
        .ok()
        .and_then(crate::catalog::builtin_identity::builtin_array_oid)
        .map_or(0, i64::from)
}
