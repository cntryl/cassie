//! Selected `pg_depend` rows backed by sequence ownership metadata.

use crate::catalog::{relation_belongs_to_database, Catalog, SequenceMeta};
use crate::types::{DataType, ObjectIdKind, Value};

use super::VirtualRow;

const PG_CLASS_OID: i64 = 1259;

pub(super) fn schema() -> Vec<(String, DataType)> {
    vec![
        oid("classid"),
        oid("objid"),
        int("objsubid"),
        oid("refclassid"),
        oid("refobjid"),
        int("refobjsubid"),
        ("deptype".to_owned(), DataType::Char { length: Some(1) }),
    ]
}

pub(super) fn rows(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    catalog
        .list_sequences()
        .into_iter()
        .filter(|sequence| {
            current_database
                .is_none_or(|database| relation_belongs_to_database(&sequence.name, database))
        })
        .filter_map(|sequence| dependency_row(catalog, &sequence, current_database))
        .collect()
}

fn dependency_row(
    catalog: &Catalog,
    sequence: &SequenceMeta,
    current_database: Option<&str>,
) -> Option<VirtualRow> {
    let owner = sequence.owned_by.as_ref()?;
    if current_database
        .is_some_and(|database| !relation_belongs_to_database(&owner.table, database))
    {
        return None;
    }

    catalog.get_collection(&owner.table)?;
    let schema = catalog.get_schema(&owner.table)?;
    let field_ordinal = schema
        .fields
        .iter()
        .position(|field| field.name == owner.column)?
        .checked_add(1)?;
    let field_ordinal = i64::try_from(field_ordinal).ok()?;
    let owner_oid = super::virtual_views_pg::relation_oid(catalog, &owner.table);
    let sequence_oid = super::virtual_views_pg::relation_oid(catalog, &sequence.name);
    let identity = catalog
        .get_constraint(&owner.table, &owner.column)
        .is_some_and(|constraint| constraint.identity.is_some());

    Some(vec![
        oid_value("classid", PG_CLASS_OID),
        oid_value("objid", sequence_oid),
        int_value("objsubid", 0),
        oid_value("refclassid", PG_CLASS_OID),
        oid_value("refobjid", owner_oid),
        int_value("refobjsubid", field_ordinal),
        string_value("deptype", if identity { "i" } else { "a" }),
    ])
}

fn oid(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::ObjectId(ObjectIdKind::Oid))
}

fn int(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::Int)
}

fn oid_value(name: &str, value: i64) -> (String, Value) {
    (name.to_owned(), Value::Int64(value))
}

fn int_value(name: &str, value: i64) -> (String, Value) {
    (name.to_owned(), Value::Int64(value))
}

fn string_value(name: &str, value: &str) -> (String, Value) {
    (name.to_owned(), Value::String(value.to_owned()))
}
