use std::collections::BTreeSet;

use crate::catalog::{
    local_name, relation_belongs_to_database, relation_schema_name, Catalog, FieldConstraint,
    IndexKind,
};
use crate::types::{DataType, Value};

use super::VirtualRow;

pub(super) fn table_constraints_schema() -> Vec<(String, DataType)> {
    vec![
        text("table_schema"),
        text("table_name"),
        text("constraint_name"),
        text("constraint_type"),
    ]
}

pub(super) fn key_column_usage_schema() -> Vec<(String, DataType)> {
    vec![
        text("table_schema"),
        text("table_name"),
        text("column_name"),
        text("constraint_name"),
        int("ordinal_position"),
        int("position_in_unique_constraint"),
    ]
}

pub(super) fn referential_constraints_schema() -> Vec<(String, DataType)> {
    vec![
        text("constraint_schema"),
        text("constraint_name"),
        text("unique_constraint_schema"),
        text("unique_constraint_name"),
        text("match_option"),
        text("update_rule"),
        text("delete_rule"),
    ]
}

pub(super) fn pg_constraint_schema() -> Vec<(String, DataType)> {
    vec![
        int("oid"),
        text("conname"),
        text("conrelid"),
        int("conrelid_oid"),
        text("contype"),
        int("connamespace_oid"),
        text("conkey"),
        text("confrelid"),
        int("confrelid_oid"),
        text("confkey"),
        bool("convalidated"),
        bool("condeferrable"),
        bool("condeferred"),
    ]
}

pub(super) fn table_constraints(
    catalog: &Catalog,
    current_database: Option<&str>,
) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for collection in catalog.list_collections() {
        if current_database
            .is_some_and(|database| !relation_belongs_to_database(&collection.name, database))
        {
            continue;
        }
        for constraint in catalog.get_constraints(&collection.name) {
            if constraint.primary_key {
                push_table_constraint(
                    &mut rows,
                    &mut seen,
                    &collection.name,
                    constraint_name(&collection.name, &constraint, ConstraintKind::PrimaryKey),
                    "PRIMARY KEY",
                );
            }
            if constraint.unique && !constraint.primary_key {
                push_table_constraint(
                    &mut rows,
                    &mut seen,
                    &collection.name,
                    constraint_name(&collection.name, &constraint, ConstraintKind::Unique),
                    "UNIQUE",
                );
            }
            if constraint.check.is_some() {
                push_table_constraint(
                    &mut rows,
                    &mut seen,
                    &collection.name,
                    constraint_name(&collection.name, &constraint, ConstraintKind::Check),
                    "CHECK",
                );
            }
            if constraint.references_table.is_some() && constraint.references_field.is_some() {
                push_table_constraint(
                    &mut rows,
                    &mut seen,
                    &collection.name,
                    constraint_name(&collection.name, &constraint, ConstraintKind::ForeignKey),
                    "FOREIGN KEY",
                );
            }
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn key_column_usage(
    catalog: &Catalog,
    current_database: Option<&str>,
) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        if current_database
            .is_some_and(|database| !relation_belongs_to_database(&collection.name, database))
        {
            continue;
        }
        for constraint in catalog.get_constraints(&collection.name) {
            if constraint.primary_key {
                rows.push(key_column_usage_row(
                    &collection.name,
                    &constraint.field,
                    constraint_name(&collection.name, &constraint, ConstraintKind::PrimaryKey),
                    constraint.primary_key_ordinal.unwrap_or(1),
                    None,
                ));
            }
            if constraint.unique && !constraint.primary_key {
                rows.push(key_column_usage_row(
                    &collection.name,
                    &constraint.field,
                    constraint_name(&collection.name, &constraint, ConstraintKind::Unique),
                    constraint.unique_ordinal.unwrap_or(1),
                    None,
                ));
            }
            if constraint.references_table.is_some() && constraint.references_field.is_some() {
                let ordinal = constraint.foreign_key_ordinal.unwrap_or(1);
                rows.push(key_column_usage_row(
                    &collection.name,
                    &constraint.field,
                    constraint_name(&collection.name, &constraint, ConstraintKind::ForeignKey),
                    ordinal,
                    Some(ordinal),
                ));
            }
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn referential_constraints(
    catalog: &Catalog,
    current_database: Option<&str>,
) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        if current_database
            .is_some_and(|database| !relation_belongs_to_database(&collection.name, database))
        {
            continue;
        }
        for constraint in catalog.get_constraints(&collection.name) {
            if constraint.references_table.is_none() || constraint.references_field.is_none() {
                continue;
            }
            let referenced_table = constraint.references_table.as_deref().unwrap_or_default();
            rows.push(vec![
                string("constraint_schema", relation_schema_name(&collection.name)),
                string(
                    "constraint_name",
                    constraint_name(&collection.name, &constraint, ConstraintKind::ForeignKey),
                ),
                string(
                    "unique_constraint_schema",
                    relation_schema_name(referenced_table),
                ),
                string(
                    "unique_constraint_name",
                    referenced_unique_constraint_name(catalog, &constraint),
                ),
                string("match_option", "NONE"),
                string(
                    "update_rule",
                    constraint
                        .foreign_key_on_update
                        .clone()
                        .unwrap_or_else(|| "NO ACTION".to_string()),
                ),
                string(
                    "delete_rule",
                    constraint
                        .foreign_key_on_delete
                        .clone()
                        .unwrap_or_else(|| "NO ACTION".to_string()),
                ),
            ]);
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn pg_constraint(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        if current_database
            .is_some_and(|database| !relation_belongs_to_database(&collection.name, database))
        {
            continue;
        }
        for constraint in catalog.get_constraints(&collection.name) {
            if constraint.primary_key {
                rows.push(pg_constraint_row(
                    catalog,
                    &collection.name,
                    &constraint.field,
                    constraint_name(&collection.name, &constraint, ConstraintKind::PrimaryKey),
                    "p",
                    None,
                    None,
                ));
            }
            if constraint.unique && !constraint.primary_key {
                rows.push(pg_constraint_row(
                    catalog,
                    &collection.name,
                    &constraint.field,
                    constraint_name(&collection.name, &constraint, ConstraintKind::Unique),
                    "u",
                    None,
                    None,
                ));
            }
            if constraint.check.is_some() {
                rows.push(pg_constraint_row(
                    catalog,
                    &collection.name,
                    &constraint.field,
                    constraint_name(&collection.name, &constraint, ConstraintKind::Check),
                    "c",
                    None,
                    None,
                ));
            }
            if constraint.not_null {
                rows.push(pg_constraint_row(
                    catalog,
                    &collection.name,
                    &constraint.field,
                    crate::catalog::generated_constraint_name(
                        &collection.name,
                        &constraint.field,
                        "n",
                    ),
                    "n",
                    None,
                    None,
                ));
            }
            if constraint.references_table.is_some() && constraint.references_field.is_some() {
                rows.push(pg_constraint_row(
                    catalog,
                    &collection.name,
                    &constraint.field,
                    constraint_name(&collection.name, &constraint, ConstraintKind::ForeignKey),
                    "f",
                    constraint.references_table.as_deref(),
                    constraint.references_field.as_deref(),
                ));
            }
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

fn push_table_constraint(
    rows: &mut Vec<VirtualRow>,
    seen: &mut BTreeSet<(String, String, String)>,
    collection: &str,
    constraint_name: String,
    constraint_type: &str,
) {
    if seen.insert((
        collection.to_string(),
        constraint_name.clone(),
        constraint_type.to_string(),
    )) {
        rows.push(vec![
            string("table_schema", relation_schema_name(collection)),
            string("table_name", local_name(collection)),
            string("constraint_name", constraint_name),
            string("constraint_type", constraint_type),
        ]);
    }
}

fn key_column_usage_row(
    collection: &str,
    field: &str,
    constraint_name: String,
    ordinal_position: u32,
    unique_position: Option<u32>,
) -> VirtualRow {
    vec![
        string("table_schema", relation_schema_name(collection)),
        string("table_name", local_name(collection)),
        string("column_name", field),
        string("constraint_name", constraint_name),
        int_value("ordinal_position", i64::from(ordinal_position)),
        (
            "position_in_unique_constraint".to_string(),
            unique_position.map_or(Value::Null, |value| Value::Int64(i64::from(value))),
        ),
    ]
}

fn pg_constraint_row(
    catalog: &Catalog,
    collection: &str,
    field: &str,
    constraint_name: String,
    constraint_type: &str,
    referenced_table: Option<&str>,
    referenced_field: Option<&str>,
) -> VirtualRow {
    let referenced_table = referenced_table.unwrap_or_default();
    let schema_name = relation_schema_name(collection);
    vec![
        int_value(
            "oid",
            super::virtual_views_pg::constraint_oid(collection, &constraint_name),
        ),
        string("conname", constraint_name),
        string("conrelid", local_name(collection)),
        int_value(
            "conrelid_oid",
            super::virtual_views_pg::relation_oid(collection),
        ),
        string("contype", constraint_type),
        int_value(
            "connamespace_oid",
            super::virtual_views_pg::namespace_oid(&schema_name),
        ),
        string("conkey", field_number(catalog, collection, field)),
        string("confrelid", local_name(referenced_table)),
        int_value(
            "confrelid_oid",
            if referenced_table.is_empty() {
                0
            } else {
                super::virtual_views_pg::relation_oid(referenced_table)
            },
        ),
        string(
            "confkey",
            referenced_field
                .map(|field| field_number(catalog, referenced_table, field))
                .unwrap_or_default(),
        ),
        bool_value("convalidated", true),
        bool_value("condeferrable", false),
        bool_value("condeferred", false),
    ]
}

fn field_number(catalog: &Catalog, collection: &str, field: &str) -> String {
    catalog
        .get_schema(collection)
        .and_then(|schema| {
            schema
                .fields
                .iter()
                .position(|candidate| candidate.name.eq_ignore_ascii_case(field))
                .map(|index| (index + 1).to_string())
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy)]
enum ConstraintKind {
    PrimaryKey,
    Unique,
    Check,
    ForeignKey,
}

fn constraint_name(collection: &str, constraint: &FieldConstraint, kind: ConstraintKind) -> String {
    match kind {
        ConstraintKind::PrimaryKey => constraint
            .primary_key_name
            .clone()
            .unwrap_or_else(|| fallback_constraint_name(collection, constraint, "PRIMARY KEY")),
        ConstraintKind::Unique => constraint
            .unique_name
            .clone()
            .unwrap_or_else(|| fallback_constraint_name(collection, constraint, "UNIQUE")),
        ConstraintKind::Check => constraint
            .check_name
            .clone()
            .unwrap_or_else(|| fallback_constraint_name(collection, constraint, "CHECK")),
        ConstraintKind::ForeignKey => constraint
            .foreign_key_name
            .clone()
            .unwrap_or_else(|| fallback_constraint_name(collection, constraint, "FOREIGN KEY")),
    }
}

fn fallback_constraint_name(collection: &str, constraint: &FieldConstraint, kind: &str) -> String {
    crate::catalog::generated_constraint_name(collection, &constraint.field, kind)
}

fn referenced_unique_constraint_name(catalog: &Catalog, constraint: &FieldConstraint) -> String {
    let Some(table) = constraint.references_table.as_deref() else {
        return String::new();
    };
    let Some(field) = constraint.references_field.as_deref() else {
        return String::new();
    };

    for candidate in catalog.get_constraints(table) {
        if candidate.field.eq_ignore_ascii_case(field) && candidate.primary_key {
            return constraint_name(table, &candidate, ConstraintKind::PrimaryKey);
        }
    }
    for candidate in catalog.get_constraints(table) {
        if candidate.field.eq_ignore_ascii_case(field) && candidate.unique {
            return constraint_name(table, &candidate, ConstraintKind::Unique);
        }
    }
    for index in catalog.list_indexes(table) {
        if index.unique && index.kind == IndexKind::Scalar {
            let fields = index.normalized_fields();
            if fields.len() == 1 && fields[0].eq_ignore_ascii_case(field) {
                return index.name;
            }
        }
    }

    crate::catalog::generated_constraint_name(table, field, "PRIMARY KEY")
}

fn text(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Text)
}

fn int(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Int)
}

fn string(name: &str, value: impl Into<String>) -> (String, Value) {
    (name.to_string(), Value::String(value.into()))
}

fn int_value(name: &str, value: i64) -> (String, Value) {
    (name.to_string(), Value::Int64(value))
}

fn bool(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Boolean)
}

fn bool_value(name: &str, value: bool) -> (String, Value) {
    (name.to_string(), Value::Bool(value))
}

fn row_sort_key(row: &VirtualRow) -> String {
    row.iter()
        .map(|(_, value)| match value {
            Value::String(value) => value.clone(),
            Value::Int64(value) => value.to_string(),
            Value::Null => String::new(),
            Value::Bool(value) => value.to_string(),
            Value::Float64(value) => value.to_string(),
            Value::Vector(value) => format!("{:?}", value.values),
            Value::Json(value) => value.to_string(),
        })
        .collect::<Vec<_>>()
        .join("|")
}
