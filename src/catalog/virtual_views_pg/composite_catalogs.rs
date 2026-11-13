//! `pg_type`, `pg_class` and `pg_attribute` rows for user composite types (#828).
use crate::catalog::{canonical_schema_name, Catalog, CompositeTypeMeta};
use crate::types::DataType;

use super::{
    attribute_row, class_row, int_value, string, type_row_with_oid, AttributeSpec, VirtualRow,
};

fn visible(catalog: &Catalog, current_database: Option<&str>) -> Vec<CompositeTypeMeta> {
    current_database.map_or_else(
        || catalog.list_composite_types(),
        |database| catalog.list_composite_types_for_database(database),
    )
}

fn namespace(metadata: &CompositeTypeMeta) -> String {
    canonical_schema_name(&metadata.database, &metadata.schema_name())
}

/// Replaces the value of column `name` in `row`.
fn replace(row: &mut VirtualRow, name: &str, value: (String, crate::types::Value)) {
    if let Some(slot) = row.iter_mut().find(|(column, _)| column == name) {
        *slot = value;
    }
}

/// The composite type row and its `_name` array type row for each type.
pub(super) fn type_rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for metadata in visible(catalog, current_database) {
        let descriptor = metadata.descriptor();
        let namespace = namespace(&metadata);
        let type_oid = i64::from(metadata.type_oid());
        let array_oid = i64::from(metadata.array_oid());
        let composite = DataType::Composite(descriptor.clone());
        let mut row = type_row_with_oid(catalog, &composite, &namespace, type_oid);
        replace(&mut row, "typarray", int_value("typarray", array_oid));
        replace(
            &mut row,
            "typrelid",
            int_value("typrelid", i64::from(metadata.relation_oid())),
        );
        rows.push(row);

        let array = DataType::Array(Box::new(composite));
        let mut row = type_row_with_oid(catalog, &array, &namespace, array_oid);
        replace(
            &mut row,
            "typname",
            string("typname", format!("_{}", metadata.local_name())),
        );
        replace(&mut row, "typtype", string("typtype", "b"));
        replace(&mut row, "typelem", int_value("typelem", type_oid));
        replace(&mut row, "typarray", int_value("typarray", 0));
        rows.push(row);
    }
    rows.extend(relation_type_rows(catalog, current_database));
    rows
}

/// The `relkind = 'c'` `pg_class` row of each composite type.
pub(super) fn class_rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    visible(catalog, current_database)
        .into_iter()
        .map(|metadata| {
            let mut row = class_row(catalog, &metadata.name, "c", None, current_database);
            replace(
                &mut row,
                "oid",
                int_value("oid", i64::from(metadata.relation_oid())),
            );
            replace(
                &mut row,
                "reltype",
                int_value("reltype", i64::from(metadata.type_oid())),
            );
            row
        })
        .collect()
}

/// One `pg_attribute` row per field of each composite type.
pub(super) fn attribute_rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for metadata in visible(catalog, current_database) {
        // A dropped attribute keeps its physical slot, so live attnums can skip it.
        let slots = metadata.descriptor().physical_positions();
        for (field, slot) in metadata.fields.iter().zip(slots) {
            let mut row = attribute_row(
                catalog,
                &metadata.name,
                &AttributeSpec {
                    field_name: &field.name,
                    data_type: &field.data_type,
                    attnum: i64::try_from(slot + 1).unwrap_or(i64::MAX),
                    constraint: None,
                    is_dropped: false,
                },
                current_database,
            );
            replace(
                &mut row,
                "attrelid_oid",
                int_value("attrelid_oid", i64::from(metadata.relation_oid())),
            );
            rows.push(row);
        }
    }
    rows
}

/// The row type of each table and its `_name` array type, as `pg_type` rows
/// with `typtype = 'c'` and `typrelid` set to the table.
fn relation_type_rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for (table, _, descriptor) in catalog.list_relation_row_types(current_database) {
        let namespace = crate::catalog::RelationId::parse_canonical(&table).map_or_else(
            || crate::catalog::relation_schema_name(&table),
            |relation| canonical_schema_name(&relation.database, &relation.schema),
        );
        let composite = DataType::Composite(descriptor.clone());
        let mut row = type_row_with_oid(catalog, &composite, &namespace, i64::from(descriptor.oid));
        replace(
            &mut row,
            "typarray",
            int_value("typarray", i64::from(descriptor.array_oid)),
        );
        replace(
            &mut row,
            "typrelid",
            int_value("typrelid", super::relation_oid(catalog, &table)),
        );
        rows.push(row);

        let array = DataType::Array(Box::new(composite));
        let mut row =
            type_row_with_oid(catalog, &array, &namespace, i64::from(descriptor.array_oid));
        replace(
            &mut row,
            "typname",
            string("typname", format!("_{}", descriptor.name)),
        );
        replace(&mut row, "typtype", string("typtype", "b"));
        replace(
            &mut row,
            "typelem",
            int_value("typelem", i64::from(descriptor.oid)),
        );
        replace(&mut row, "typarray", int_value("typarray", 0));
        rows.push(row);
    }
    rows
}

/// `pg_class.reltype`: the derived row type of a table, `0` for every other
/// relation kind (views have no row type here; see the type contract).
pub(super) fn relation_reltype(catalog: &Catalog, kind: &str, name: &str) -> i64 {
    if kind != "r" {
        return 0;
    }
    catalog
        .relation_row_type(name)
        .map_or(0, |row_type| i64::from(row_type.oid))
}
