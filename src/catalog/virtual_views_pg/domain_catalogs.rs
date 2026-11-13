//! `pg_type` rows for user domain types (#828).
use crate::catalog::{canonical_schema_name, Catalog, DomainTypeMeta};
use crate::types::DataType;

use super::{bool_value, int_value, string, type_row_with_oid, VirtualRow};

fn visible(catalog: &Catalog, current_database: Option<&str>) -> Vec<DomainTypeMeta> {
    current_database.map_or_else(
        || catalog.list_domain_types(),
        |database| catalog.list_domain_types_for_database(database),
    )
}

/// Replaces the value of column `name` in `row`.
fn replace(row: &mut VirtualRow, name: &str, value: (String, crate::types::Value)) {
    if let Some(slot) = row.iter_mut().find(|(column, _)| column == name) {
        *slot = value;
    }
}

/// The `typtype = 'd'` row and `_name` array row of each domain.
pub(super) fn type_rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for metadata in visible(catalog, current_database) {
        let namespace = canonical_schema_name(&metadata.database, &metadata.schema_name());
        let type_oid = i64::from(metadata.type_oid());
        let array_oid = i64::from(metadata.array_oid());
        let base_oid = metadata.base.type_oid();
        let mut row = type_row_with_oid(catalog, &metadata.base, &namespace, type_oid);
        replace(
            &mut row,
            "typname",
            string("typname", metadata.local_name()),
        );
        replace(&mut row, "typtype", string("typtype", "d"));
        replace(&mut row, "typbasetype", int_value("typbasetype", base_oid));
        replace(
            &mut row,
            "typnotnull",
            bool_value("typnotnull", metadata.not_null),
        );
        replace(
            &mut row,
            "typtypmod",
            int_value("typtypmod", i64::from(metadata.base.atttypmod())),
        );
        replace(&mut row, "typarray", int_value("typarray", array_oid));
        replace(&mut row, "typelem", int_value("typelem", 0));
        rows.push(row);

        let array = DataType::Array(Box::new(metadata.base.clone()));
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
    rows
}

/// Reports the domain, not its base type, as the `atttypid` of a domain column.
pub(super) fn mark_domain_column(row: &mut VirtualRow, domain: &crate::types::DomainRef) {
    replace(
        row,
        "atttypid",
        int_value("atttypid", i64::from(domain.oid)),
    );
    // PostgreSQL keeps the base typmod on the domain, not on its columns.
    replace(row, "atttypmod", int_value("atttypmod", -1));
}
