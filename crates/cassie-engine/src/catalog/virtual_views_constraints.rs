use std::collections::BTreeSet;

use crate::app::CassieSession;
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
        super::virtual_views_pg::oid("conrelid"),
        int("conrelid_oid"),
        text("contype"),
        super::virtual_views_pg::oid("conindid"),
        int("conindid_oid"),
        int("connamespace_oid"),
        text("conkey"),
        super::virtual_views_pg::oid("confrelid"),
        int("confrelid_oid"),
        text("confkey"),
        bool("conenforced"),
        bool("convalidated"),
        bool("condeferrable"),
        bool("condeferred"),
        super::virtual_views_pg::oid("conparentid"),
        bool("conperiod"),
    ]
}

pub(super) fn table_constraints(
    catalog: &Catalog,
    current_database: Option<&str>,
) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for collection in catalog.list_collections_canonical() {
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
                    &constraint_name(&collection.name, &constraint, ConstraintKind::PrimaryKey),
                    "PRIMARY KEY",
                );
            }
            if constraint.unique && !constraint.primary_key {
                push_table_constraint(
                    &mut rows,
                    &mut seen,
                    &collection.name,
                    &constraint_name(&collection.name, &constraint, ConstraintKind::Unique),
                    "UNIQUE",
                );
            }
            if constraint.check.is_some() {
                push_table_constraint(
                    &mut rows,
                    &mut seen,
                    &collection.name,
                    &constraint_name(&collection.name, &constraint, ConstraintKind::Check),
                    "CHECK",
                );
            }
            if constraint.references_table.is_some() && constraint.references_field.is_some() {
                push_table_constraint(
                    &mut rows,
                    &mut seen,
                    &collection.name,
                    &constraint_name(&collection.name, &constraint, ConstraintKind::ForeignKey),
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
    for collection in catalog.list_collections_canonical() {
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
                    &constraint_name(&collection.name, &constraint, ConstraintKind::PrimaryKey),
                    constraint.primary_key_ordinal.unwrap_or(1),
                    None,
                ));
            }
            if constraint.unique && !constraint.primary_key {
                rows.push(key_column_usage_row(
                    &collection.name,
                    &constraint.field,
                    &constraint_name(&collection.name, &constraint, ConstraintKind::Unique),
                    constraint.unique_ordinal.unwrap_or(1),
                    None,
                ));
            }
            if constraint.references_table.is_some() && constraint.references_field.is_some() {
                let ordinal = constraint.foreign_key_ordinal.unwrap_or(1);
                rows.push(key_column_usage_row(
                    &collection.name,
                    &constraint.field,
                    &constraint_name(&collection.name, &constraint, ConstraintKind::ForeignKey),
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
    for collection in catalog.list_collections_canonical() {
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
    for collection in catalog.list_collections_canonical() {
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
                    &pg_constraint_name(&collection.name, &constraint, ConstraintKind::PrimaryKey),
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
                    &pg_constraint_name(&collection.name, &constraint, ConstraintKind::Unique),
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
                    &pg_constraint_name(&collection.name, &constraint, ConstraintKind::Check),
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
                    &not_null_constraint_name(&collection.name, &constraint.field),
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
                    &pg_constraint_name(&collection.name, &constraint, ConstraintKind::ForeignKey),
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

pub(super) fn definition_by_oid(
    catalog: &Catalog,
    current_database: &str,
    oid: i64,
    session: Option<&CassieSession>,
) -> Option<String> {
    for collection in catalog.list_collections_canonical() {
        if !relation_belongs_to_database(&collection.name, current_database)
            || session.is_some_and(|session| {
                !crate::app::relation_readable(catalog, current_database, session, &collection.name)
            })
        {
            continue;
        }
        let constraints = catalog.get_constraints(&collection.name);
        for constraint in &constraints {
            for kind in [
                ConstraintKind::PrimaryKey,
                ConstraintKind::Unique,
                ConstraintKind::Check,
                ConstraintKind::ForeignKey,
            ] {
                let Some(name) = constraint_kind_name(&collection.name, constraint, kind) else {
                    continue;
                };
                if super::virtual_views_pg::constraint_oid(&collection.name, &name) == oid {
                    return render_constraint_definition(
                        &constraints,
                        &collection.name,
                        kind,
                        &name,
                    );
                }
            }
        }
    }
    None
}

fn constraint_kind_name(
    collection: &str,
    constraint: &FieldConstraint,
    kind: ConstraintKind,
) -> Option<String> {
    match kind {
        ConstraintKind::PrimaryKey => constraint
            .primary_key
            .then(|| pg_constraint_name(collection, constraint, ConstraintKind::PrimaryKey)),
        ConstraintKind::Unique => (constraint.unique && !constraint.primary_key)
            .then(|| pg_constraint_name(collection, constraint, ConstraintKind::Unique)),
        ConstraintKind::Check => constraint
            .check
            .as_ref()
            .map(|_| pg_constraint_name(collection, constraint, ConstraintKind::Check)),
        ConstraintKind::ForeignKey => constraint
            .references_table
            .as_ref()
            .zip(constraint.references_field.as_ref())
            .map(|_| pg_constraint_name(collection, constraint, ConstraintKind::ForeignKey)),
    }
}

fn render_constraint_definition(
    constraints: &[FieldConstraint],
    collection: &str,
    kind: ConstraintKind,
    name: &str,
) -> Option<String> {
    let members = constraints
        .iter()
        .filter(|constraint| {
            constraint_kind_name(collection, constraint, kind).as_deref() == Some(name)
        })
        .collect::<Vec<_>>();
    let fields = members
        .iter()
        .map(|constraint| constraint.field.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    match kind {
        ConstraintKind::PrimaryKey => Some(format!("PRIMARY KEY ({fields})")),
        ConstraintKind::Unique => Some(format!("UNIQUE ({fields})")),
        ConstraintKind::Check => members.first().and_then(|constraint| {
            constraint.check.as_ref().map(|check| {
                let operator = match check.operator {
                    crate::catalog::ConstraintOperator::Eq => "=",
                    crate::catalog::ConstraintOperator::NotEq => "<>",
                    crate::catalog::ConstraintOperator::Lt => "<",
                    crate::catalog::ConstraintOperator::Lte => "<=",
                    crate::catalog::ConstraintOperator::Gt => ">",
                    crate::catalog::ConstraintOperator::Gte => ">=",
                    crate::catalog::ConstraintOperator::Like => "LIKE",
                    crate::catalog::ConstraintOperator::IsDistinctFrom => "IS DISTINCT FROM",
                    crate::catalog::ConstraintOperator::IsNotDistinctFrom => "IS NOT DISTINCT FROM",
                };
                format!(
                    "CHECK (({} {operator} {}))",
                    check.field,
                    constraint_value_sql(&check.value)
                )
            })
        }),
        ConstraintKind::ForeignKey => members.first().and_then(|constraint| {
            Some(format!(
                "FOREIGN KEY ({fields}) REFERENCES {}({})",
                local_name(constraint.references_table.as_deref()?),
                members
                    .iter()
                    .filter_map(|member| member.references_field.as_deref())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }),
    }
}

fn constraint_value_sql(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_owned(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::String(value) => format!("'{}'", value.replace('\'', "''")),
        value => value.to_string(),
    }
}

fn push_table_constraint(
    rows: &mut Vec<VirtualRow>,
    seen: &mut BTreeSet<(String, String, String)>,
    collection: &str,
    constraint_name: &str,
    constraint_type: &str,
) {
    if seen.insert((
        collection.to_string(),
        constraint_name.to_string(),
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
    constraint_name: &str,
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
    constraint_name: &str,
    constraint_type: &str,
    referenced_table: Option<&str>,
    referenced_field: Option<&str>,
) -> VirtualRow {
    let referenced_table = referenced_table.unwrap_or_default();
    let schema_name = relation_schema_name(collection);
    vec![
        int_value(
            "oid",
            super::virtual_views_pg::constraint_oid(collection, constraint_name),
        ),
        string("conname", constraint_name.to_string()),
        int_value(
            "conrelid",
            super::virtual_views_pg::relation_oid(catalog, collection),
        ),
        int_value(
            "conrelid_oid",
            super::virtual_views_pg::relation_oid(catalog, collection),
        ),
        string("contype", constraint_type),
        (
            "conindid".to_string(),
            if matches!(constraint_type, "p" | "u") {
                catalog
                    .get_index(collection, constraint_name)
                    .map_or(Value::Int64(0), |index| {
                        Value::Int64(super::virtual_views_pg::index_oid(&index))
                    })
            } else {
                Value::Int64(0)
            },
        ),
        (
            "conindid_oid".to_string(),
            if matches!(constraint_type, "p" | "u") {
                catalog
                    .get_index(collection, constraint_name)
                    .map_or(Value::Int64(0), |index| {
                        Value::Int64(super::virtual_views_pg::index_oid(&index))
                    })
            } else {
                Value::Int64(0)
            },
        ),
        int_value(
            "connamespace_oid",
            super::virtual_views_pg::namespace_oid(
                catalog,
                &crate::catalog::RelationId::parse_canonical(collection).map_or_else(
                    || schema_name.clone(),
                    |relation| {
                        crate::catalog::canonical_schema_name(&relation.database, &relation.schema)
                    },
                ),
            ),
        ),
        string("conkey", field_number(catalog, collection, field)),
        int_value(
            "confrelid",
            if referenced_table.is_empty() {
                0
            } else {
                super::virtual_views_pg::relation_oid(catalog, referenced_table)
            },
        ),
        int_value(
            "confrelid_oid",
            if referenced_table.is_empty() {
                0
            } else {
                super::virtual_views_pg::relation_oid(catalog, referenced_table)
            },
        ),
        string(
            "confkey",
            referenced_field.map_or_default(|field| field_number(catalog, referenced_table, field)),
        ),
        bool_value("conenforced", true),
        bool_value("convalidated", true),
        bool_value("condeferrable", false),
        bool_value("condeferred", false),
        int_value("conparentid", 0),
        bool_value("conperiod", false),
    ]
}

fn not_null_constraint_name(collection: &str, field: &str) -> String {
    format!("{}_{}_not_null", local_name(collection), field)
}

fn field_number(catalog: &Catalog, collection: &str, field: &str) -> String {
    catalog
        .get_schema(collection)
        .and_then(|schema| {
            schema
                .fields
                .iter()
                .position(|candidate| {
                    crate::sql::ColumnIdentifierPath::stored_field_key(&candidate.name)
                        == crate::sql::ColumnIdentifierPath::reference_field_key(field)
                })
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

fn pg_constraint_name(
    collection: &str,
    constraint: &FieldConstraint,
    kind: ConstraintKind,
) -> String {
    match kind {
        ConstraintKind::PrimaryKey => constraint
            .primary_key_name
            .clone()
            .or_else(|| constraint.constraint_index_name.clone())
            .unwrap_or_else(|| format!("{}_pkey", local_name(collection))),
        ConstraintKind::Unique => constraint
            .unique_name
            .clone()
            .or_else(|| constraint.constraint_index_name.clone())
            .unwrap_or_else(|| {
                crate::catalog::generated_constraint_name(collection, &constraint.field, "UNIQUE")
            }),
        ConstraintKind::Check | ConstraintKind::ForeignKey => {
            constraint_name(collection, constraint, kind)
        }
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
        if crate::sql::ColumnIdentifierPath::stored_field_key(&candidate.field)
            == crate::sql::ColumnIdentifierPath::reference_field_key(field)
            && candidate.primary_key
        {
            return constraint_name(table, &candidate, ConstraintKind::PrimaryKey);
        }
    }
    for candidate in catalog.get_constraints(table) {
        if crate::sql::ColumnIdentifierPath::stored_field_key(&candidate.field)
            == crate::sql::ColumnIdentifierPath::reference_field_key(field)
            && candidate.unique
        {
            return constraint_name(table, &candidate, ConstraintKind::Unique);
        }
    }
    for index in catalog.list_indexes(table) {
        if index.unique && index.kind == IndexKind::Scalar {
            let fields = index.normalized_fields();
            if fields.len() == 1
                && crate::sql::ColumnIdentifierPath::stored_field_key(&fields[0])
                    == crate::sql::ColumnIdentifierPath::reference_field_key(field)
            {
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
            Value::Array(value) => value.to_storage_json().to_string(),
            Value::Typed(value) => value.text(),
        })
        .collect::<Vec<_>>()
        .join("|")
}
