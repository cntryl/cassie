use crate::catalog::{Catalog, FieldConstraint, IndexMeta};
use crate::types::{DataType, Value};

use super::VirtualRow;

pub(super) fn namespace_schema() -> Vec<(String, DataType)> {
    vec![int("oid"), text("nspname"), int("nspowner"), text("nspacl")]
}

pub(super) fn class_schema() -> Vec<(String, DataType)> {
    vec![
        int("oid"),
        text("relname"),
        text("relkind"),
        text("relnamespace"),
        int("relnamespace_oid"),
        int("relowner"),
        bool("relhasindex"),
        text("relpersistence"),
        int("reltuples"),
        text("relacl"),
    ]
}

pub(super) fn attribute_schema() -> Vec<(String, DataType)> {
    vec![
        text("attrelid"),
        int("attrelid_oid"),
        text("attname"),
        int("attnum"),
        int("atttypid"),
        bool("attnotnull"),
        int("atttypmod"),
        bool("atthasdef"),
        bool("attisdropped"),
        text("attidentity"),
        text("attgenerated"),
        int("attcollation"),
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
        int("indexrelid_oid"),
        int("indrelid_oid"),
        bool("indisunique"),
        bool("indisprimary"),
        text("indkey"),
        int("indnatts"),
        int("indnkeyatts"),
        bool("indisvalid"),
        bool("indisready"),
        text("indpred"),
        text("indexprs"),
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
        namespace_row("information_schema"),
        namespace_row("pg_catalog"),
        namespace_row("public"),
    ];
    rows.extend(
        catalog
            .list_namespaces()
            .into_iter()
            .map(|namespace| namespace_row(&namespace.name)),
    );
    rows.sort_by_key(row_sort_key);
    rows.dedup_by_key(|row| row_sort_key(row));
    rows
}

pub(super) fn pg_class(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_collections()
        .into_iter()
        .map(|collection| class_row(catalog, &collection.name, "r"))
        .collect::<Vec<_>>();
    rows.extend(
        catalog
            .list_views()
            .into_iter()
            .map(|view| class_row(catalog, &view.name, "v")),
    );
    rows.extend(
        catalog
            .list_sequences()
            .into_iter()
            .map(|sequence| class_row(catalog, &sequence.name, "S")),
    );
    rows.extend(pg_indexes(catalog).into_iter().map(|index| {
        let indexname = lookup_string(&index, "indexname");
        class_row(catalog, &indexname, "i")
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
                int_value("indexrelid_oid", index_oid(&index.name)),
                int_value("indrelid_oid", relation_oid(&index.collection)),
                bool_value("indisunique", index.unique),
                bool_value("indisprimary", primary),
                string("indkey", index_keys(catalog, &index)),
                int_value("indnatts", index_key_count(catalog, &index)),
                int_value("indnkeyatts", index_key_count(catalog, &index)),
                bool_value("indisvalid", true),
                bool_value("indisready", true),
                string("indpred", index.predicate.clone().unwrap_or_default()),
                string("indexprs", index.normalized_expressions().join(", ")),
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
                int_value("adnum", index + 1 ),
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
        int_value("attrelid_oid", relation_oid(relation)),
        string("attname", field_name),
        int_value("attnum", index + 1 ),
        int_value("atttypid", data_type.type_oid()),
        bool_value("attnotnull", is_not_null(constraint)),
        int_value("atttypmod", i64::from(data_type.atttypmod())),
        bool_value("atthasdef", constraint_has_default(constraint)),
        bool_value("attisdropped", false),
        string("attidentity", ""),
        string("attgenerated", ""),
        int_value("attcollation", 0),
    ]
}

fn namespace_row(name: &str) -> VirtualRow {
    vec![
        int_value("oid", namespace_oid(name)),
        string("nspname", name),
        int_value("nspowner", postgres_role_oid()),
        string("nspacl", ""),
    ]
}

fn class_row(catalog: &Catalog, name: &str, kind: &str) -> VirtualRow {
    vec![
        int_value("oid", relation_kind_oid(kind, name)),
        string("relname", name),
        string("relkind", kind),
        string("relnamespace", "public"),
        int_value("relnamespace_oid", namespace_oid("public")),
        int_value("relowner", postgres_role_oid()),
        bool_value("relhasindex", relation_has_indexes(catalog, name)),
        string("relpersistence", "p"),
        int_value("reltuples", 0),
        string("relacl", ""),
    ]
}

fn relation_has_indexes(catalog: &Catalog, name: &str) -> bool {
    sorted_indexes(catalog)
        .into_iter()
        .any(|index| index.collection.eq_ignore_ascii_case(name))
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
                .position(|candidate| candidate.name.eq_ignore_ascii_case(&field)).map_or_else(|| "0".to_string(), |position| (position + 1).to_string())
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

fn index_key_count(catalog: &Catalog, index: &IndexMeta) -> usize {
    let field_count = index.normalized_fields().len();
    let expression_count = index.normalized_expressions().len();
    if field_count + expression_count > 0 {
        field_count + expression_count
    } else {
        index_keys(catalog, index)
            .split_whitespace()
            .filter(|key| !key.is_empty())
            .count()
    }
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
        .is_some_and(|constraint| constraint.not_null || constraint.primary_key)
}

fn constraint_has_default(constraint: Option<&FieldConstraint>) -> bool {
    constraint
        .is_some_and(|constraint| {
            constraint.default_expression.is_some() || constraint.default_value.is_some()
        })
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
        DataType::Boolean => 1,
        DataType::SmallInt => 2,
        DataType::Int | DataType::Date => 4,
        DataType::BigInt | DataType::Float | DataType::Time | DataType::Timestamp => 8,
        DataType::Uuid => 16,
        DataType::Null | DataType::Char { .. } | DataType::Varchar { .. } | DataType::Text | DataType::Bytea | DataType::Json | DataType::Vector(_) | DataType::Array(_) => -1,
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

pub(super) fn postgres_role_oid() -> i64 {
    10
}

pub(super) fn namespace_oid(name: &str) -> i64 {
    match name.to_ascii_lowercase().as_str() {
        "pg_catalog" => 11,
        "public" => 2200,
        "information_schema" => 13428,
        other => stable_catalog_oid("namespace", other, 50_000),
    }
}

pub(super) fn relation_oid(name: &str) -> i64 {
    stable_catalog_oid("relation", name, 100_000)
}

pub(super) fn index_oid(name: &str) -> i64 {
    stable_catalog_oid("index", name, 200_000)
}

pub(super) fn constraint_oid(collection: &str, constraint: &str) -> i64 {
    stable_catalog_oid("constraint", &format!("{collection}.{constraint}"), 300_000)
}

fn relation_kind_oid(kind: &str, name: &str) -> i64 {
    if kind == "i" {
        index_oid(name)
    } else {
        relation_oid(name)
    }
}

fn stable_catalog_oid(kind: &str, name: &str, base: i64) -> i64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in kind
        .bytes()
        .chain([b':'])
        .chain(name.to_ascii_lowercase().bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    base + i64::try_from(hash % 800_000).unwrap_or(0)
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

fn int_value<T>(name: &str, value: T) -> (String, Value)
where
    T: TryInto<i64>,
{
    (
        name.to_string(),
        Value::Int64(value.try_into().unwrap_or(i64::MAX)),
    )
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
