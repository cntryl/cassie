use crate::catalog::{Catalog, FieldConstraint, IndexMeta};
use crate::types::{DataType, Value};

use super::VirtualRow;

pub(super) fn namespace_schema() -> Vec<(String, DataType)> {
    vec![text("nspname")]
}

pub(super) fn class_schema() -> Vec<(String, DataType)> {
    vec![text("relname"), text("relkind"), text("relnamespace")]
}

pub(super) fn attribute_schema() -> Vec<(String, DataType)> {
    vec![
        text("attrelid"),
        text("attname"),
        int("attnum"),
        int("atttypid"),
        bool("attnotnull"),
        int("atttypmod"),
        bool("atthasdef"),
    ]
}

pub(super) fn indexes_schema() -> Vec<(String, DataType)> {
    vec![
        text("schemaname"),
        text("tablename"),
        text("indexname"),
        text("indexdef"),
    ]
}

pub(super) fn index_schema() -> Vec<(String, DataType)> {
    vec![
        text("indexrelid"),
        text("indrelid"),
        bool("indisunique"),
        bool("indisprimary"),
        text("indkey"),
    ]
}

pub(super) fn attrdef_schema() -> Vec<(String, DataType)> {
    vec![text("adrelid"), int("adnum"), text("adsrc")]
}

pub(super) fn type_schema() -> Vec<(String, DataType)> {
    vec![
        int("oid"),
        text("typname"),
        text("typnamespace"),
        int("typlen"),
        bool("typbyval"),
        text("typtype"),
        text("typcategory"),
        int("typelem"),
    ]
}

pub(super) fn pg_namespace(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = vec![
        vec![string("nspname", "information_schema")],
        vec![string("nspname", "pg_catalog")],
        vec![string("nspname", "public")],
    ];
    rows.extend(
        catalog
            .list_namespaces()
            .into_iter()
            .map(|namespace| vec![string("nspname", namespace.name)]),
    );
    rows.sort_by_key(row_sort_key);
    rows.dedup_by_key(|row| row_sort_key(row));
    rows
}

pub(super) fn pg_class(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_collections()
        .into_iter()
        .map(|collection| {
            vec![
                string("relname", collection.name),
                string("relkind", "r"),
                string("relnamespace", "public"),
            ]
        })
        .collect::<Vec<_>>();
    rows.extend(catalog.list_views().into_iter().map(|view| {
        vec![
            string("relname", view.name),
            string("relkind", "v"),
            string("relnamespace", "public"),
        ]
    }));
    rows.extend(catalog.list_sequences().into_iter().map(|sequence| {
        vec![
            string("relname", sequence.name),
            string("relkind", "S"),
            string("relnamespace", "public"),
        ]
    }));
    rows.extend(pg_indexes(catalog).into_iter().map(|index| {
        let indexname = lookup_string(&index, "indexname");
        vec![
            string("relname", indexname),
            string("relkind", "i"),
            string("relnamespace", "public"),
        ]
    }));
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn pg_attribute(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        let Some(schema) = catalog.get_schema(&collection.name) else {
            continue;
        };
        let constraints = catalog.get_constraints(&collection.name);
        for (index, field) in schema.fields.iter().enumerate() {
            let constraint = constraint_for_field(&constraints, &field.name);
            rows.push(attribute_row(
                &schema.collection,
                &field.name,
                &field.data_type,
                index,
                constraint,
            ));
        }
    }
    for view in catalog.list_views() {
        for (index, field) in view.schema.fields.iter().enumerate() {
            rows.push(attribute_row(
                &view.name,
                &field.name,
                &field.data_type,
                index,
                None,
            ));
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn pg_indexes(catalog: &Catalog) -> Vec<VirtualRow> {
    sorted_indexes(catalog)
        .into_iter()
        .map(|index| {
            vec![
                string("schemaname", "public"),
                string("tablename", &index.collection),
                string("indexname", &index.name),
                string("indexdef", index_definition(&index)),
            ]
        })
        .collect()
}

pub(super) fn pg_index(catalog: &Catalog) -> Vec<VirtualRow> {
    sorted_indexes(catalog)
        .into_iter()
        .map(|index| {
            let primary = index_is_primary(catalog, &index);
            vec![
                string("indexrelid", &index.name),
                string("indrelid", &index.collection),
                bool_value("indisunique", index.unique),
                bool_value("indisprimary", primary),
                string("indkey", index_keys(catalog, &index)),
            ]
        })
        .collect()
}

pub(super) fn pg_attrdef(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        let Some(schema) = catalog.get_schema(&collection.name) else {
            continue;
        };
        let constraints = catalog.get_constraints(&collection.name);
        for (index, field) in schema.fields.iter().enumerate() {
            let Some(constraint) = constraint_for_field(&constraints, &field.name) else {
                continue;
            };
            let Some(default) = constraint_default_expression(constraint) else {
                continue;
            };
            rows.push(vec![
                string("adrelid", &schema.collection),
                int_value("adnum", (index + 1) as i64),
                string("adsrc", default),
            ]);
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

pub(super) fn pg_type(_catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = builtin_type_rows();
    rows.sort_by_key(row_sort_key);
    rows
}

fn attribute_row(
    relation: &str,
    field_name: &str,
    data_type: &DataType,
    index: usize,
    constraint: Option<&FieldConstraint>,
) -> VirtualRow {
    vec![
        string("attrelid", relation),
        string("attname", field_name),
        int_value("attnum", (index + 1) as i64),
        int_value("atttypid", data_type.type_oid()),
        bool_value("attnotnull", is_not_null(constraint)),
        int_value("atttypmod", data_type.atttypmod() as i64),
        bool_value("atthasdef", constraint_has_default(constraint)),
    ]
}

fn sorted_indexes(catalog: &Catalog) -> Vec<IndexMeta> {
    let mut indexes = catalog.indexes.read().values().cloned().collect::<Vec<_>>();
    indexes.sort_by_key(|index| {
        (
            index.collection.to_ascii_lowercase(),
            index.name.to_ascii_lowercase(),
        )
    });
    indexes
}

fn index_definition(index: &IndexMeta) -> String {
    let include = if index.include_fields.is_empty() {
        String::new()
    } else {
        format!(" INCLUDE ({})", index.include_fields.join(", "))
    };
    let predicate = index
        .predicate
        .as_ref()
        .map(|predicate| format!(" WHERE {predicate}"))
        .unwrap_or_default();
    let keys = index
        .normalized_fields()
        .into_iter()
        .chain(index.normalized_expressions())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE {}INDEX {} ON {} ({})",
        if index.unique { "UNIQUE " } else { "" },
        index.name,
        index.collection,
        keys
    ) + &include
        + &predicate
}

fn index_is_primary(catalog: &Catalog, index: &IndexMeta) -> bool {
    catalog
        .get_constraints(&index.collection)
        .into_iter()
        .any(|constraint| {
            constraint.primary_key
                && constraint
                    .primary_key_name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&index.name))
        })
}

fn index_keys(catalog: &Catalog, index: &IndexMeta) -> String {
    let Some(schema) = catalog.get_schema(&index.collection) else {
        return String::new();
    };
    let keys = index
        .normalized_fields()
        .into_iter()
        .map(|field| {
            schema
                .fields
                .iter()
                .position(|candidate| candidate.name.eq_ignore_ascii_case(&field))
                .map(|position| (position + 1).to_string())
                .unwrap_or_else(|| "0".to_string())
        })
        .chain(
            index
                .normalized_expressions()
                .into_iter()
                .map(|_| "0".to_string()),
        )
        .collect::<Vec<_>>();
    keys.join(" ")
}

fn constraint_for_field<'a>(
    constraints: &'a [FieldConstraint],
    field: &str,
) -> Option<&'a FieldConstraint> {
    constraints
        .iter()
        .find(|constraint| constraint.field.eq_ignore_ascii_case(field))
}

fn is_not_null(constraint: Option<&FieldConstraint>) -> bool {
    constraint
        .map(|constraint| constraint.not_null || constraint.primary_key)
        .unwrap_or(false)
}

fn constraint_has_default(constraint: Option<&FieldConstraint>) -> bool {
    constraint
        .map(|constraint| {
            constraint.default_expression.is_some() || constraint.default_value.is_some()
        })
        .unwrap_or(false)
}

pub(super) fn constraint_default_expression(constraint: &FieldConstraint) -> Option<String> {
    if let Some(expression) = constraint.default_expression.as_ref() {
        return Some(expression.clone());
    }
    constraint.default_value.as_ref().map(default_expression)
}

fn default_expression(value: &serde_json::Value) -> String {
    if value.is_null() {
        return "NULL".to_string();
    }
    if let Some(value) = value.as_str() {
        return format!("'{}'", value.replace('\'', "''"));
    }
    value.to_string()
}

fn builtin_type_rows() -> Vec<VirtualRow> {
    let namespace = "pg_catalog";
    let mut rows = vec![
        type_row(DataType::Null, namespace),
        type_row(DataType::Boolean, namespace),
        type_row(DataType::SmallInt, namespace),
        type_row(DataType::Int, namespace),
        type_row(DataType::BigInt, namespace),
        type_row(DataType::Float, namespace),
        type_row(DataType::Text, namespace),
        type_row(DataType::Char { length: Some(1) }, namespace),
        type_row(DataType::Varchar { length: Some(8) }, namespace),
        type_row(DataType::Bytea, namespace),
        type_row(DataType::Uuid, namespace),
        type_row(DataType::Date, namespace),
        type_row(DataType::Time, namespace),
        type_row(DataType::Timestamp, namespace),
        type_row(DataType::Json, namespace),
        type_row(DataType::Vector(2), namespace),
    ];

    rows.extend([
        type_row(DataType::Array(Box::new(DataType::Boolean)), namespace),
        type_row(DataType::Array(Box::new(DataType::SmallInt)), namespace),
        type_row(DataType::Array(Box::new(DataType::Int)), namespace),
        type_row(DataType::Array(Box::new(DataType::BigInt)), namespace),
        type_row(DataType::Array(Box::new(DataType::Float)), namespace),
        type_row(DataType::Array(Box::new(DataType::Text)), namespace),
        type_row(
            DataType::Array(Box::new(DataType::Char { length: Some(1) })),
            namespace,
        ),
        type_row(
            DataType::Array(Box::new(DataType::Varchar { length: Some(8) })),
            namespace,
        ),
        type_row(DataType::Array(Box::new(DataType::Bytea)), namespace),
        type_row(DataType::Array(Box::new(DataType::Uuid)), namespace),
        type_row(DataType::Array(Box::new(DataType::Json)), namespace),
    ]);

    rows
}

fn type_row(data_type: DataType, namespace: &str) -> VirtualRow {
    let typname = data_type.type_name();
    vec![
        int_value("oid", data_type.type_oid()),
        string("typname", typname),
        string("typnamespace", namespace),
        int_value("typlen", type_length(&data_type)),
        bool_value("typbyval", is_type_passed_by_value(&data_type)),
        string("typtype", type_kind(&data_type)),
        string("typcategory", type_category(&data_type)),
        int_value("typelem", element_type_oid(&data_type)),
    ]
}

fn type_length(data_type: &DataType) -> i64 {
    match data_type {
        DataType::Null => -1,
        DataType::Boolean => 1,
        DataType::SmallInt => 2,
        DataType::Int => 4,
        DataType::BigInt => 8,
        DataType::Float => 8,
        DataType::Uuid => 16,
        DataType::Char { .. } => -1,
        DataType::Varchar { .. } => -1,
        DataType::Date => 4,
        DataType::Time => 8,
        DataType::Timestamp => 8,
        DataType::Text => -1,
        DataType::Bytea => -1,
        DataType::Json => -1,
        DataType::Vector(_) => -1,
        DataType::Array(_) => -1,
    }
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
    )
}

fn type_kind(data_type: &DataType) -> String {
    if matches!(data_type, DataType::Array(_)) {
        "a".to_string()
    } else {
        "b".to_string()
    }
}

fn type_category(data_type: &DataType) -> String {
    match data_type {
        DataType::Null => "Z".to_string(),
        DataType::Boolean => "B".to_string(),
        DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float => "N".to_string(),
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } | DataType::Json => {
            "S".to_string()
        }
        DataType::Bytea | DataType::Uuid => "U".to_string(),
        DataType::Date | DataType::Time | DataType::Timestamp => "D".to_string(),
        DataType::Vector(_) => "V".to_string(),
        DataType::Array(_) => "A".to_string(),
    }
}

fn element_type_oid(data_type: &DataType) -> i64 {
    match data_type {
        DataType::Array(inner) => inner.type_oid(),
        _ => 0,
    }
}

fn text(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Text)
}

fn int(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Int)
}

fn bool(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Boolean)
}

fn string(name: &str, value: impl Into<String>) -> (String, Value) {
    (name.to_string(), Value::String(value.into()))
}

fn int_value(name: &str, value: i64) -> (String, Value) {
    (name.to_string(), Value::Int64(value))
}

fn bool_value(name: &str, value: bool) -> (String, Value) {
    (name.to_string(), Value::Bool(value))
}

fn lookup_string(row: &[(String, Value)], name: &str) -> String {
    row.iter()
        .find_map(|(column, value)| {
            if column == name {
                if let Value::String(value) = value {
                    return Some(value.clone());
                }
            }
            None
        })
        .unwrap_or_default()
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
        .join("\u{1f}")
}
