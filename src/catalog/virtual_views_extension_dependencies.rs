//! Extension-membership `pg_depend` rows for the selected built-in profiles (#1273).
//!
//! Only objects that Cassie already publishes in `pg_proc`, `pg_type` or
//! `pg_operator` are listed, using the same stored object identities, so a
//! `pg_depend.objid` always joins its catalog row. Nothing is emitted for an
//! extension that is not installed in the requested database.

use crate::catalog::object_identity::{normal_oid, stored_namespace_oid};
use crate::catalog::{Catalog, ExtensionMeta};
use crate::types::Value;

use super::{int_value, oid_value, string_value, VirtualRow};

const PG_TYPE_OID: i64 = 1247;
const PG_PROC_OID: i64 = 1255;
const PG_OPERATOR_OID: i64 = 2617;
const PG_EXTENSION_OID: i64 = 3079;

pub(super) fn rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let extensions = current_database.map_or_else(
        || catalog.list_extensions(),
        |database| catalog.list_extensions_for_database(database),
    );
    let mut rows = extensions
        .iter()
        .flat_map(|extension| extension_rows(catalog, extension))
        .collect::<Vec<_>>();
    rows.sort_by_key(sort_key);
    rows
}

fn extension_rows(catalog: &Catalog, extension: &ExtensionMeta) -> Vec<VirtualRow> {
    let Some(extension_oid) = normal_oid(extension.object_id) else {
        return Vec::new();
    };
    if stored_namespace_oid(catalog, &extension.schema).is_none() {
        return Vec::new();
    }
    members(extension)
        .into_iter()
        .filter_map(|(class_oid, object_id)| {
            let member_oid = normal_oid(object_id)?;
            Some(member_row(
                class_oid,
                i64::from(member_oid),
                i64::from(extension_oid),
            ))
        })
        .collect()
}

/// The `(catalog class OID, stored object id)` pairs an installed extension owns.
fn members(extension: &ExtensionMeta) -> Vec<(i64, u64)> {
    let mut members = Vec::new();
    if extension.name.eq_ignore_ascii_case("pgcrypto") {
        members.extend(
            [
                extension.digest_text_object_id,
                extension.digest_bytea_object_id,
            ]
            .into_iter()
            .flatten()
            .map(|object_id| (PG_PROC_OID, object_id)),
        );
    }
    if extension.name.eq_ignore_ascii_case("vector") {
        members.extend(
            extension
                .vector_type_object_id
                .map(|object_id| (PG_TYPE_OID, object_id)),
        );
        if let Some(identities) = &extension.vector_catalog_identities {
            members.extend(
                [
                    identities.operators.l2,
                    identities.operators.inner_product,
                    identities.operators.cosine,
                ]
                .into_iter()
                .map(|object_id| (PG_OPERATOR_OID, object_id)),
            );
        }
    }
    members
}

fn member_row(class_oid: i64, member_oid: i64, extension_oid: i64) -> VirtualRow {
    vec![
        oid_value("classid", class_oid),
        oid_value("objid", member_oid),
        int_value("objsubid", 0),
        oid_value("refclassid", PG_EXTENSION_OID),
        oid_value("refobjid", extension_oid),
        int_value("refobjsubid", 0),
        string_value("deptype", "e"),
    ]
}

fn sort_key(row: &VirtualRow) -> (i64, i64, i64) {
    let value = |name: &str| {
        row.iter()
            .find_map(|(column, value)| match value {
                Value::Int64(number) if column == name => Some(*number),
                _ => None,
            })
            .unwrap_or_default()
    };
    (value("refobjid"), value("classid"), value("objid"))
}
