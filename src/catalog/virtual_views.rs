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
        "information_schema.views" => vec![
            text("table_schema"),
            text("table_name"),
            text("view_definition"),
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
            int("atttypid"),
        ],
        "pg_catalog.pg_indexes" => vec![
            text("schemaname"),
            text("tablename"),
            text("indexname"),
            text("indexdef"),
        ],
        "pg_catalog.pg_constraint" => vec![text("conname"), text("conrelid"), text("contype")],
        "pg_catalog.pg_roles" => vec![text("rolname"), bool("rolcanlogin"), bool("rolsuper")],
        "pg_catalog.pg_rollups" => vec![
            text("rollup_name"),
            text("source_collection"),
            text("output_collection"),
            text("state"),
            int("lag_rows"),
            text("bucket_expr"),
        ],
        "pg_catalog.pg_projection_checkpoints" => vec![
            text("projection_id"),
            text("collection"),
            text("kind"),
            text("source_identity"),
            text("source_checkpoint"),
            int("source_position"),
            text("last_applied_event_id"),
            text("replay_batch_id"),
            int("lag"),
            text("freshness"),
            text("last_error"),
        ],
        "pg_catalog.pg_materialized_projections" => vec![
            text("projection_name"),
            text("state"),
            text("active_version"),
            text("output_collection"),
            text("source_collections"),
            int("schema_epoch"),
            text("last_error"),
        ],
        "pg_catalog.pg_projection_versions" => vec![
            text("projection_name"),
            text("version_id"),
            text("output_collection"),
            text("state"),
            int("created_ms"),
            int("activated_ms"),
            int("retired_ms"),
            text("last_error"),
        ],
        "pg_catalog.pg_retention_policies" => vec![
            text("policy_name"),
            text("collection"),
            text("timestamp_field"),
            text("retention_duration"),
            text("enforcement_mode"),
            text("state"),
            int("last_enforced_ms"),
            int("last_deleted_rows"),
            int("last_skipped_rows"),
            text("last_error"),
        ],
        "pg_catalog.pg_type" => vec![
            int("oid"),
            text("typname"),
            text("typnamespace"),
            int("typlen"),
            bool("typbyval"),
            text("typtype"),
            text("typcategory"),
            int("typelem"),
        ],
        _ => return None,
    };
    Some(fields)
}

pub fn rows(catalog: &Catalog, name: &str) -> Option<Vec<VirtualRow>> {
    let name = normalize_name(name);
    let rows = match name.as_str() {
        "information_schema.tables" => information_schema_tables(catalog),
        "information_schema.columns" => information_schema_columns(catalog),
        "information_schema.views" => information_schema_views(catalog),
        "information_schema.table_constraints" => information_schema_table_constraints(catalog),
        "information_schema.key_column_usage" => information_schema_key_column_usage(catalog),
        "pg_catalog.pg_namespace" => pg_namespace(catalog),
        "pg_catalog.pg_class" => pg_class(catalog),
        "pg_catalog.pg_attribute" => pg_attribute(catalog),
        "pg_catalog.pg_indexes" => pg_indexes(catalog),
        "pg_catalog.pg_constraint" => pg_constraint(catalog),
        "pg_catalog.pg_type" => pg_type(catalog),
        "pg_catalog.pg_roles" => catalog
            .list_roles()
            .into_iter()
            .map(|role| {
                vec![
                    ("rolname".to_string(), Value::String(role.name)),
                    ("rolcanlogin".to_string(), Value::Bool(role.can_login)),
                    ("rolsuper".to_string(), Value::Bool(role.is_admin)),
                ]
            })
            .collect(),
        "pg_catalog.pg_rollups" => catalog
            .list_rollups()
            .into_iter()
            .map(|rollup| {
                vec![
                    string("rollup_name", rollup.name),
                    string("source_collection", rollup.source_collection),
                    string("output_collection", rollup.output_collection),
                    string("state", rollup.state.as_str()),
                    int_value("lag_rows", rollup.refresh_cursor.lag_rows as i64),
                    string("bucket_expr", rollup.bucket_expr),
                ]
            })
            .collect(),
        "pg_catalog.pg_projection_checkpoints" => catalog
            .list_projection_metadata()
            .into_iter()
            .map(|projection| {
                vec![
                    string("projection_id", projection.projection_id().to_string()),
                    string("collection", projection.collection),
                    string("kind", projection.kind.as_str()),
                    string(
                        "source_identity",
                        projection.source_identity.unwrap_or_default(),
                    ),
                    string(
                        "source_checkpoint",
                        projection.source_checkpoint.unwrap_or_default(),
                    ),
                    int_value(
                        "source_position",
                        projection.source_position.unwrap_or_default() as i64,
                    ),
                    string(
                        "last_applied_event_id",
                        projection.last_applied_event_id.unwrap_or_default(),
                    ),
                    string(
                        "replay_batch_id",
                        projection.replay_batch_id.unwrap_or_default(),
                    ),
                    int_value("lag", projection.lag as i64),
                    string("freshness", projection.freshness.as_str()),
                    string("last_error", projection.last_error.unwrap_or_default()),
                ]
            })
            .collect(),
        "pg_catalog.pg_materialized_projections" => catalog
            .list_projection_metadata()
            .into_iter()
            .filter_map(|projection| {
                let materialized = projection.materialized.clone()?;
                let active_version = projection.active_version.clone().unwrap_or_default();
                let output_collection = projection
                    .active_output_collection()
                    .unwrap_or(&materialized.output_collection)
                    .to_string();
                let last_error = projection.last_error.clone().unwrap_or_default();
                Some(vec![
                    string("projection_name", projection.collection),
                    string("state", materialized.state.as_str()),
                    string("active_version", active_version),
                    string("output_collection", output_collection),
                    string(
                        "source_collections",
                        materialized.source_collections.join(","),
                    ),
                    int_value("schema_epoch", materialized.schema_epoch as i64),
                    string("last_error", last_error),
                ])
            })
            .collect(),
        "pg_catalog.pg_projection_versions" => catalog
            .list_projection_metadata()
            .into_iter()
            .flat_map(|projection| {
                let projection_name = projection.collection.clone();
                projection.versions.into_iter().map(move |version| {
                    vec![
                        string("projection_name", projection_name.clone()),
                        string("version_id", version.version_id),
                        string("output_collection", version.output_collection),
                        string("state", version.state.as_str()),
                        int_value("created_ms", version.created_ms as i64),
                        int_value(
                            "activated_ms",
                            version.activated_ms.unwrap_or_default() as i64,
                        ),
                        int_value("retired_ms", version.retired_ms.unwrap_or_default() as i64),
                        string("last_error", version.last_error.unwrap_or_default()),
                    ]
                })
            })
            .collect(),
        "pg_catalog.pg_retention_policies" => catalog
            .list_retention_policies()
            .into_iter()
            .map(|policy| {
                vec![
                    string("policy_name", policy.name),
                    string("collection", policy.collection),
                    string("timestamp_field", policy.timestamp_field),
                    string("retention_duration", policy.retention_duration),
                    string("enforcement_mode", policy.enforcement_mode.as_str()),
                    string("state", policy.state.as_str()),
                    int_value(
                        "last_enforced_ms",
                        policy.last_enforced_ms.unwrap_or_default() as i64,
                    ),
                    int_value("last_deleted_rows", policy.last_deleted_rows as i64),
                    int_value("last_skipped_rows", policy.last_skipped_rows as i64),
                    string("last_error", policy.last_error.unwrap_or_default()),
                ]
            })
            .collect(),
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

fn bool(name: &str) -> (String, DataType) {
    (name.to_string(), DataType::Boolean)
}

fn pg_type(_catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = builtin_type_rows();
    rows.sort_by_key(row_sort_key);
    rows
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
        DataType::Bytea => "U".to_string(),
        DataType::Uuid => "U".to_string(),
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

fn bool_value(name: &str, value: bool) -> (String, Value) {
    (name.to_string(), Value::Bool(value))
}

fn information_schema_tables(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_collections()
        .into_iter()
        .map(|collection| {
            vec![
                string("table_schema", "public"),
                string("table_name", collection.name),
                string("table_type", "BASE TABLE"),
            ]
        })
        .collect::<Vec<_>>();

    rows.extend(catalog.list_views().into_iter().map(|view| {
        vec![
            string("table_schema", "public"),
            string("table_name", view.name),
            string("table_type", "VIEW"),
        ]
    }));

    rows.sort_by_key(row_sort_key);
    rows
}

fn information_schema_columns(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        let Some(schema) = catalog.get_schema(&collection.name) else {
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
    for view in catalog.list_views() {
        for (index, field) in view.schema.fields.iter().enumerate() {
            rows.push(vec![
                string("table_schema", "public"),
                string("table_name", &view.name),
                string("column_name", &field.name),
                string("data_type", data_type_name(&field.data_type)),
                int_value("ordinal_position", (index + 1) as i64),
                string("is_nullable", "YES"),
            ]);
        }
    }
    rows
}

fn information_schema_views(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_views()
        .into_iter()
        .map(|view| {
            vec![
                string("table_schema", "public"),
                string("table_name", view.name),
                string("view_definition", view.query),
            ]
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(row_sort_key);
    rows
}

fn information_schema_table_constraints(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        for constraint in catalog.get_constraints(&collection.name) {
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
            if constraint.references_table.is_some() && constraint.references_field.is_some() {
                rows.push(table_constraint_row(
                    &collection.name,
                    &constraint.field,
                    "FOREIGN KEY",
                ));
            }
        }
    }
    rows.sort_by_key(row_sort_key);
    rows
}

fn information_schema_key_column_usage(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        for constraint in catalog.get_constraints(&collection.name) {
            if constraint.primary_key || constraint.unique || constraint.references_table.is_some()
            {
                rows.push(vec![
                    string("table_schema", "public"),
                    string("table_name", &collection.name),
                    string("column_name", &constraint.field),
                    string(
                        "constraint_name",
                        crate::catalog::generated_constraint_name(
                            &collection.name,
                            &constraint.field,
                            if constraint.primary_key {
                                "primary_key"
                            } else if constraint.references_table.is_some() {
                                "foreign_key"
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

fn pg_namespace(catalog: &Catalog) -> Vec<VirtualRow> {
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

fn pg_class(catalog: &Catalog) -> Vec<VirtualRow> {
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

fn pg_attribute(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        let Some(schema) = catalog.get_schema(&collection.name) else {
            continue;
        };
        for (index, field) in schema.fields.iter().enumerate() {
            rows.push(vec![
                string("attrelid", &schema.collection),
                string("attname", &field.name),
                int_value("attnum", (index + 1) as i64),
                int_value("atttypid", field.data_type.type_oid()),
            ]);
        }
    }
    for view in catalog.list_views() {
        for (index, field) in view.schema.fields.iter().enumerate() {
            rows.push(vec![
                string("attrelid", &view.name),
                string("attname", &field.name),
                int_value("attnum", (index + 1) as i64),
                int_value("atttypid", field.data_type.type_oid()),
            ]);
        }
    }
    rows
}

fn pg_indexes(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut indexes = catalog.indexes.read().values().cloned().collect::<Vec<_>>();
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
                string("indexdef", {
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
                }),
            ]
        })
        .collect()
}

fn pg_constraint(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for collection in catalog.list_collections() {
        for constraint in catalog.get_constraints(&collection.name) {
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
            if constraint.references_table.is_some() && constraint.references_field.is_some() {
                rows.push(pg_constraint_row(&collection.name, &constraint.field, "f"));
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
            crate::catalog::generated_constraint_name(collection, field, constraint_type),
        ),
        string("constraint_type", constraint_type),
    ]
}

fn pg_constraint_row(collection: &str, field: &str, constraint_type: &str) -> VirtualRow {
    vec![
        string(
            "conname",
            crate::catalog::generated_constraint_name(collection, field, constraint_type),
        ),
        string("conrelid", collection),
        string("contype", constraint_type),
    ]
}

fn data_type_name(data_type: &DataType) -> String {
    match data_type {
        DataType::Null => "null".to_string(),
        DataType::SmallInt => "smallint".to_string(),
        DataType::Int => "int".to_string(),
        DataType::BigInt => "bigint".to_string(),
        DataType::Float => "float".to_string(),
        DataType::Boolean => "boolean".to_string(),
        DataType::Text => "text".to_string(),
        DataType::Char { length } => match length {
            Some(length) => format!("char({length})"),
            None => "char".to_string(),
        },
        DataType::Varchar { length } => match length {
            Some(length) => format!("varchar({length})"),
            None => "varchar".to_string(),
        },
        DataType::Bytea => "bytea".to_string(),
        DataType::Uuid => "uuid".to_string(),
        DataType::Date => "date".to_string(),
        DataType::Time => "time".to_string(),
        DataType::Timestamp => "timestamp".to_string(),
        DataType::Vector(dimensions) => format!("vector({dimensions})"),
        DataType::Json => "json".to_string(),
        DataType::Array(inner) => format!("{}[]", inner.type_name()),
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
