use crate::catalog::Catalog;
use crate::types::{DataType, Value};

pub type VirtualRow = Vec<(String, Value)>;

pub fn schema(name: &str) -> Option<Vec<(String, DataType)>> {
    let name = normalize_name(name);
    let fields = match name.as_str() {
        "information_schema.tables" => {
            vec![text("table_schema"), text("table_name"), text("table_type")]
        }
        "information_schema.columns" => vec![
            text("table_schema"),
            text("table_name"),
            text("column_name"),
            text("data_type"),
            int("ordinal_position"),
            text("is_nullable"),
        ],
        "information_schema.table_constraints" => vec![
            text("table_schema"),
            text("table_name"),
            text("constraint_name"),
            text("constraint_type"),
        ],
        "information_schema.key_column_usage" => vec![
            text("table_schema"),
            text("table_name"),
            text("column_name"),
            text("constraint_name"),
        ],
        "pg_catalog.pg_namespace" => vec![text("nspname")],
        "pg_catalog.pg_class" => vec![text("relname"), text("relkind"), text("relnamespace")],
        "pg_catalog.pg_attribute" => vec![
            text("attrelid"),
            text("attname"),
            int("attnum"),
            text("atttypid"),
        ],
        "pg_catalog.pg_indexes" => vec![
            text("schemaname"),
            text("tablename"),
            text("indexname"),
            text("indexdef"),
        ],
        "pg_catalog.pg_constraint" => vec![text("conname"), text("conrelid"), text("contype")],
        "pg_catalog.pg_roles" => vec![text("rolname")],
        _ => return None,
    };
    Some(fields)
}

pub async fn rows(catalog: &Catalog, name: &str) -> Option<Vec<VirtualRow>> {
    let name = normalize_name(name);
    let rows = match name.as_str() {
        "information_schema.tables" => information_schema_tables(catalog).await,
        "information_schema.columns" => information_schema_columns(catalog).await,
        "information_schema.table_constraints" => {
            information_schema_table_constraints(catalog).await
        }
        "information_schema.key_column_usage" => information_schema_key_column_usage(catalog).await,
        "pg_catalog.pg_namespace" => pg_namespace(catalog).await,
        "pg_catalog.pg_class" => pg_class(catalog).await,
        "pg_catalog.pg_attribute" => pg_attribute(catalog).await,
        "pg_catalog.pg_indexes" => pg_indexes(catalog).await,
        "pg_catalog.pg_constraint" => pg_constraint(catalog).await,
        "pg_catalog.pg_roles" => Vec::new(),
        _ => return None,
    };
    Some(rows)
}

fn normalize_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn text(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Text)
}

fn int(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Int)
}

async fn information_schema_tables(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_collections()
        .await
        .into_iter()
        .map(|collection| {
            vec![
                string("table_schema", "public"),
                string("table_name", collection.name),
                string("table_type", "BASE TABLE"),
            ]
        })
        .collect()
}

async fn information_schema_columns(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections().await {
        let Some(schema) = catalog.get_schema(&collection.name).await else {
            continue;
        };
        for (index, field) in schema.fields.iter().enumerate() {
            rows.push(vec![
                string("table_schema", "public"),
                string("table_name", &schema.collection),
                string("column_name", &field.name),
                string("data_type", data_type_name(&field.data_type)),
                int_value("ordinal_position", (index + 1) as i64),
                string("is_nullable", "YES"),
            ]);
        }
    }
    rows
}

async fn information_schema_table_constraints(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections().await {
        for constraint in catalog.get_constraints(&collection.name).await {
            if constraint.primary_key {
                rows.push(table_constraint_row(
                    &collection.name,
                    &constraint.field,
                    "PRIMARY KEY",
                ));
            }
            if constraint.unique {
                rows.push(table_constraint_row(
                    &collection.name,
                    &constraint.field,
                    "UNIQUE",
                ));
            }
            if constraint.check.is_some() {
                rows.push(table_constraint_row(
                    &collection.name,
                    &constraint.field,
                    "CHECK",
                ));
            }
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

async fn information_schema_key_column_usage(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections().await {
        for constraint in catalog.get_constraints(&collection.name).await {
            if constraint.primary_key || constraint.unique {
                rows.push(vec![
                    string("table_schema", "public"),
                    string("table_name", &collection.name),
                    string("column_name", &constraint.field),
                    string(
                        "constraint_name",
                        constraint_name(
                            &collection.name,
                            &constraint.field,
                            if constraint.primary_key {
                                "primary_key"
                            } else {
                                "unique"
                            },
                        ),
                    ),
                ]);
            }
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

async fn pg_namespace(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = vec![
        vec![string("nspname", "information_schema")],
        vec![string("nspname", "pg_catalog")],
        vec![string("nspname", "public")],
    ];
    rows.extend(
        catalog
            .list_namespaces()
            .await
            .into_iter()
            .map(|namespace| vec![string("nspname", namespace.name)]),
    );
    rows.sort_by_key(row_sort_key);
    rows.dedup_by_key(|row| row_sort_key(row));
    rows
}

async fn pg_class(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_collections()
        .await
        .into_iter()
        .map(|collection| {
            vec![
                string("relname", collection.name),
                string("relkind", "r"),
                string("relnamespace", "public"),
            ]
        })
        .collect::<Vec<_>>();
    rows.extend(pg_indexes(catalog).await.into_iter().map(|index| {
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

async fn pg_attribute(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections().await {
        let Some(schema) = catalog.get_schema(&collection.name).await else {
            continue;
        };
        for (index, field) in schema.fields.iter().enumerate() {
            rows.push(vec![
                string("attrelid", &schema.collection),
                string("attname", &field.name),
                int_value("attnum", (index + 1) as i64),
                string("atttypid", data_type_name(&field.data_type)),
            ]);
        }
    }
    rows
}

async fn pg_indexes(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut indexes = catalog
        .indexes
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    indexes.sort_by_key(|index| {
        (
            index.collection.to_ascii_lowercase(),
            index.name.to_ascii_lowercase(),
        )
    });

    indexes
        .into_iter()
        .map(|index| {
            vec![
                string("schemaname", "public"),
                string("tablename", &index.collection),
                string("indexname", &index.name),
                string(
                    "indexdef",
                    format!(
                        "CREATE {}INDEX {} ON {} ({})",
                        if index.unique { "UNIQUE " } else { "" },
                        index.name,
                        index.collection,
                        index.field
                    ),
                ),
            ]
        })
        .collect()
}

async fn pg_constraint(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections().await {
        for constraint in catalog.get_constraints(&collection.name).await {
            if constraint.primary_key {
                rows.push(pg_constraint_row(&collection.name, &constraint.field, "p"));
            }
            if constraint.unique {
                rows.push(pg_constraint_row(&collection.name, &constraint.field, "u"));
            }
            if constraint.check.is_some() {
                rows.push(pg_constraint_row(&collection.name, &constraint.field, "c"));
            }
            if constraint.not_null {
                rows.push(pg_constraint_row(&collection.name, &constraint.field, "n"));
            }
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

fn table_constraint_row(collection: &str, field: &str, constraint_type: &str) -> VirtualRow {
    vec![
        string("table_schema", "public"),
        string("table_name", collection),
        string(
            "constraint_name",
            constraint_name(collection, field, constraint_type),
        ),
        string("constraint_type", constraint_type),
    ]
}

fn pg_constraint_row(collection: &str, field: &str, constraint_type: &str) -> VirtualRow {
    vec![
        string(
            "conname",
            constraint_name(collection, field, constraint_type),
        ),
        string("conrelid", collection),
        string("contype", constraint_type),
    ]
}

fn constraint_name(collection: &str, field: &str, kind: &str) -> String {
    format!(
        "{}_{}_{}",
        collection,
        field,
        kind.to_ascii_lowercase().replace(' ', "_")
    )
}

fn data_type_name(data_type: &DataType) -> String {
    match data_type {
        DataType::Int => "int".to_string(),
        DataType::Float => "float".to_string(),
        DataType::Boolean => "boolean".to_string(),
        DataType::Text => "text".to_string(),
        DataType::Vector(dimensions) => format!("vector({dimensions})"),
        DataType::Json => "json".to_string(),
    }
}

fn string(name: &str, value: impl Into<String>) -> (String, Value) {
    (name.to_string(), Value::String(value.into()))
}

fn int_value(name: &str, value: i64) -> (String, Value) {
    (name.to_string(), Value::Int64(value))
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
        .join("|")
}
