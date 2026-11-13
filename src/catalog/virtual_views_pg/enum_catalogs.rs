//! `pg_type` and `pg_enum` rows for user enum types (#827).
use crate::catalog::{canonical_schema_name, Catalog, EnumTypeMeta};
use crate::types::{DataType, Value};

use super::{bool_value, int_value, string, type_row_with_oid, VirtualRow};

fn visible(catalog: &Catalog, current_database: Option<&str>) -> Vec<EnumTypeMeta> {
    current_database.map_or_else(
        || catalog.list_enum_types(),
        |database| catalog.list_enum_types_for_database(database),
    )
}

/// Replaces the value of column `name` in `row`.
fn replace(row: &mut VirtualRow, name: &str, value: (String, Value)) {
    if let Some(slot) = row.iter_mut().find(|(column, _)| column == name) {
        *slot = value;
    }
}

/// The `typtype = 'e'` row and `_name` array row of each enum type.
pub(super) fn type_rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for metadata in visible(catalog, current_database) {
        let namespace = canonical_schema_name(&metadata.database, &metadata.schema_name());
        let type_oid = i64::from(metadata.type_oid());
        let array_oid = i64::from(metadata.array_oid());
        let scalar = DataType::Enum(metadata.descriptor());
        let mut row = type_row_with_oid(catalog, &scalar, &namespace, type_oid);
        replace(
            &mut row,
            "typname",
            string("typname", metadata.local_name()),
        );
        // A label is stored as its 4-byte member identity.
        replace(&mut row, "typlen", int_value("typlen", 4));
        replace(&mut row, "typbyval", bool_value("typbyval", true));
        replace(&mut row, "typtype", string("typtype", "e"));
        replace(&mut row, "typcategory", string("typcategory", "E"));
        replace(&mut row, "typelem", int_value("typelem", 0));
        replace(&mut row, "typarray", int_value("typarray", array_oid));
        rows.push(row);

        let array = DataType::Array(Box::new(scalar));
        let mut row = type_row_with_oid(catalog, &array, &namespace, array_oid);
        replace(
            &mut row,
            "typname",
            string("typname", format!("_{}", metadata.local_name())),
        );
        replace(&mut row, "typlen", int_value("typlen", -1));
        replace(&mut row, "typbyval", bool_value("typbyval", false));
        replace(&mut row, "typtype", string("typtype", "b"));
        replace(&mut row, "typcategory", string("typcategory", "A"));
        replace(&mut row, "typelem", int_value("typelem", type_oid));
        replace(&mut row, "typarray", int_value("typarray", 0));
        rows.push(row);
    }
    rows
}

pub(in crate::catalog::virtual_views) fn enum_schema() -> Vec<(String, DataType)> {
    vec![
        super::oid("oid"),
        super::oid("enumtypid"),
        ("enumsortorder".to_owned(), DataType::Float),
        ("enumlabel".to_owned(), DataType::Name),
    ]
}

/// One `pg_enum` row per member of each enum type in the current database.
pub(in crate::catalog::virtual_views) fn enum_rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for metadata in visible(catalog, current_database) {
        let type_oid = i64::from(metadata.type_oid());
        for member in &metadata.members {
            rows.push(vec![
                int_value(
                    "oid",
                    crate::catalog::object_identity::normal_oid(member.member_id).unwrap_or(0),
                ),
                int_value("enumtypid", type_oid),
                (
                    "enumsortorder".to_owned(),
                    Value::Float64(f64::from(member.sort_order)),
                ),
                string("enumlabel", member.label.clone()),
            ]);
        }
    }
    rows
}

#[cfg(test)]
#[path = "enum_catalogs_tests.rs"]
mod tests;
