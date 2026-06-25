use crate::catalog::{Catalog, FieldConstraint};
use crate::types::{DataType, Value};

pub type VirtualRow = Vec<(String, Value)>;

#[path = "virtual_views_consistency.rs"]
mod virtual_views_consistency;
#[path = "virtual_views_constraints.rs"]
mod virtual_views_constraints;
#[path = "virtual_views_pg.rs"]
mod virtual_views_pg;
#[path = "virtual_views_repair.rs"]
mod virtual_views_repair;
#[path = "virtual_views_sequences.rs"]
mod virtual_views_sequences;
#[path = "virtual_views_storage.rs"]
mod virtual_views_storage;

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
            text("column_default"),
            text("udt_name"),
            int("character_maximum_length"),
            int("numeric_precision"),
            int("numeric_scale"),
            int("datetime_precision"),
        ],
        "information_schema.views" => vec![
            text("table_schema"),
            text("table_name"),
            text("view_definition"),
        ],
        "information_schema.sequences" => {
            virtual_views_sequences::information_schema_sequences_schema()
        }
        "information_schema.table_constraints" => {
            virtual_views_constraints::table_constraints_schema()
        }
        "information_schema.key_column_usage" => {
            virtual_views_constraints::key_column_usage_schema()
        }
        "information_schema.referential_constraints" => {
            virtual_views_constraints::referential_constraints_schema()
        }
        "pg_catalog.pg_namespace" => virtual_views_pg::namespace_schema(),
        "pg_catalog.pg_class" => virtual_views_pg::class_schema(),
        "pg_catalog.pg_attribute" => virtual_views_pg::attribute_schema(),
        "pg_catalog.pg_indexes" => virtual_views_pg::indexes_schema(),
        "pg_catalog.pg_index" => virtual_views_pg::index_schema(),
        "pg_catalog.pg_attrdef" => virtual_views_pg::attrdef_schema(),
        "pg_catalog.pg_table_storage" => virtual_views_storage::schema(),
        "pg_catalog.pg_constraint" => virtual_views_constraints::pg_constraint_schema(),
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
        "pg_catalog.pg_projection_operations" => vec![
            text("projection_name"),
            text("kind"),
            text("active_version"),
            int("lag"),
            text("freshness"),
            text("rebuild_state"),
            text("verification_state"),
            text("root_state"),
            text("last_error"),
        ],
        "pg_catalog.pg_projection_hashes" => vec![
            text("projection_name"),
            text("algorithm"),
            int("digest_length"),
            int("canonical_encoder_version"),
            int("hash_version"),
            text("row_state"),
            int("row_count"),
            text("range_state"),
            int("range_count"),
            text("root_state"),
            text("root_digest"),
        ],
        "pg_catalog.pg_projection_integrity_reports" => vec![
            text("projection_name"),
            text("state"),
            text("target"),
            text("version_id"),
            text("mode"),
            int("mismatch_count"),
            int("missing_count"),
            int("stale_count"),
            bool("repairable"),
            int("elapsed_ms"),
            text("checked_components"),
            text("skipped_components"),
            text("last_error"),
        ],
        "pg_catalog.pg_projection_comparison_reports" => vec![
            text("report_id"),
            text("target"),
            text("target_version_id"),
            text("state"),
            text("compatibility_status"),
            text("root_digest"),
            text("manifest_digest"),
            int("mismatch_count"),
            int("unverifiable_count"),
            text("diagnostic_sample"),
            int("created_ms"),
            text("last_error"),
        ],
        "pg_catalog.pg_projection_consistency_reports" => virtual_views_consistency::schema(),
        "pg_catalog.pg_projection_repair_reports" => virtual_views_repair::schema(),
        "pg_catalog.pg_operational_assignments" => vec![
            text("assignment_id"),
            text("node_id"),
            text("projection_id"),
            text("tenant"),
            text("partition_key"),
            int("generation"),
            text("state"),
            text("routing_hint"),
            int("updated_ms"),
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
        "pg_catalog.pg_type" => virtual_views_pg::type_schema(),
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
        "information_schema.sequences" => {
            virtual_views_sequences::information_schema_sequences(catalog)
        }
        "information_schema.table_constraints" => {
            virtual_views_constraints::table_constraints(catalog)
        }
        "information_schema.key_column_usage" => {
            virtual_views_constraints::key_column_usage(catalog)
        }
        "information_schema.referential_constraints" => {
            virtual_views_constraints::referential_constraints(catalog)
        }
        "pg_catalog.pg_namespace" => virtual_views_pg::pg_namespace(catalog),
        "pg_catalog.pg_class" => virtual_views_pg::pg_class(catalog),
        "pg_catalog.pg_attribute" => virtual_views_pg::pg_attribute(catalog),
        "pg_catalog.pg_indexes" => virtual_views_pg::pg_indexes(catalog),
        "pg_catalog.pg_index" => virtual_views_pg::pg_index(catalog),
        "pg_catalog.pg_attrdef" => virtual_views_pg::pg_attrdef(catalog),
        "pg_catalog.pg_table_storage" => virtual_views_storage::rows(catalog),
        "pg_catalog.pg_constraint" => virtual_views_constraints::pg_constraint(catalog),
        "pg_catalog.pg_type" => virtual_views_pg::pg_type(catalog),
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
        "pg_catalog.pg_projection_operations" => catalog
            .list_projection_metadata()
            .into_iter()
            .map(|projection| {
                vec![
                    string("projection_name", projection.collection),
                    string("kind", projection.kind.as_str()),
                    string(
                        "active_version",
                        projection.active_version.unwrap_or_default(),
                    ),
                    int_value("lag", projection.lag as i64),
                    string("freshness", projection.freshness.as_str()),
                    string("rebuild_state", projection.rebuild_state.as_str()),
                    string("verification_state", projection.verification.state.as_str()),
                    string("root_state", projection.hashes.root.state.as_str()),
                    string("last_error", projection.last_error.unwrap_or_default()),
                ]
            })
            .collect(),
        "pg_catalog.pg_projection_hashes" => catalog
            .list_projection_metadata()
            .into_iter()
            .map(|projection| {
                vec![
                    string("projection_name", projection.collection),
                    string("algorithm", projection.hashes.algorithm.algorithm),
                    int_value(
                        "digest_length",
                        projection.hashes.algorithm.digest_length as i64,
                    ),
                    int_value(
                        "canonical_encoder_version",
                        projection.hashes.algorithm.canonical_encoder_version as i64,
                    ),
                    int_value(
                        "hash_version",
                        projection.hashes.algorithm.hash_version as i64,
                    ),
                    string("row_state", projection.hashes.rows.state.as_str()),
                    int_value("row_count", projection.hashes.rows.row_count as i64),
                    string("range_state", projection.hashes.ranges.state.as_str()),
                    int_value("range_count", projection.hashes.ranges.range_count as i64),
                    string("root_state", projection.hashes.root.state.as_str()),
                    string(
                        "root_digest",
                        projection.hashes.root.digest.unwrap_or_default(),
                    ),
                ]
            })
            .collect(),
        "pg_catalog.pg_projection_integrity_reports" => catalog
            .list_projection_metadata()
            .into_iter()
            .map(|projection| {
                vec![
                    string("projection_name", projection.collection),
                    string("state", projection.integrity.state.as_str()),
                    string("target", projection.integrity.target.unwrap_or_default()),
                    string(
                        "version_id",
                        projection.integrity.version_id.unwrap_or_default(),
                    ),
                    string("mode", projection.integrity.mode),
                    int_value("mismatch_count", projection.integrity.mismatch_count as i64),
                    int_value("missing_count", projection.integrity.missing_count as i64),
                    int_value("stale_count", projection.integrity.stale_count as i64),
                    bool_value("repairable", projection.integrity.repairable),
                    int_value("elapsed_ms", projection.integrity.elapsed_ms as i64),
                    string(
                        "checked_components",
                        projection.integrity.checked_components.join(","),
                    ),
                    string(
                        "skipped_components",
                        projection.integrity.skipped_components.join(","),
                    ),
                    string(
                        "last_error",
                        projection.integrity.last_error.unwrap_or_default(),
                    ),
                ]
            })
            .collect(),
        "pg_catalog.pg_projection_comparison_reports" => catalog
            .list_projection_comparison_reports()
            .into_iter()
            .map(|report| {
                vec![
                    string("report_id", report.report_id),
                    string("target", report.target),
                    string(
                        "target_version_id",
                        report.target_version_id.unwrap_or_default(),
                    ),
                    string("state", report.state),
                    string("compatibility_status", report.compatibility_status),
                    string("root_digest", report.root_digest.unwrap_or_default()),
                    string(
                        "manifest_digest",
                        report.manifest_digest.unwrap_or_default(),
                    ),
                    int_value("mismatch_count", report.mismatch_count as i64),
                    int_value("unverifiable_count", report.unverifiable_count as i64),
                    string("diagnostic_sample", report.diagnostic_sample.join(",")),
                    int_value("created_ms", report.created_ms as i64),
                    string("last_error", report.last_error.unwrap_or_default()),
                ]
            })
            .collect(),
        "pg_catalog.pg_projection_consistency_reports" => virtual_views_consistency::rows(catalog),
        "pg_catalog.pg_projection_repair_reports" => virtual_views_repair::rows(catalog),
        "pg_catalog.pg_operational_assignments" => catalog
            .list_operational_assignments()
            .into_iter()
            .map(|assignment| {
                vec![
                    string("assignment_id", assignment.assignment_id),
                    string("node_id", assignment.node_id),
                    string("projection_id", assignment.projection_id),
                    string("tenant", assignment.tenant.unwrap_or_default()),
                    string(
                        "partition_key",
                        assignment.partition_key.unwrap_or_default(),
                    ),
                    int_value("generation", assignment.generation as i64),
                    string("state", assignment.state.as_str()),
                    string("routing_hint", assignment.routing_hint.unwrap_or_default()),
                    int_value("updated_ms", assignment.updated_ms as i64),
                ]
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
        let constraints = catalog.get_constraints(&collection.name);
        for (index, field) in schema.fields.iter().enumerate() {
            rows.push(information_schema_column_row(
                &schema.collection,
                &field.name,
                &field.data_type,
                index,
                constraint_for_field(&constraints, &field.name),
            ));
        }
    }
    for view in catalog.list_views() {
        for (index, field) in view.schema.fields.iter().enumerate() {
            rows.push(information_schema_column_row(
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

fn information_schema_column_row(
    relation: &str,
    field_name: &str,
    data_type: &DataType,
    index: usize,
    constraint: Option<&FieldConstraint>,
) -> VirtualRow {
    vec![
        string("table_schema", "public"),
        string("table_name", relation),
        string("column_name", field_name),
        string("data_type", data_type_name(data_type)),
        int_value("ordinal_position", (index + 1) as i64),
        string(
            "is_nullable",
            if is_not_nullable(constraint) {
                "NO"
            } else {
                "YES"
            },
        ),
        optional_string(
            "column_default",
            constraint.and_then(virtual_views_pg::constraint_default_expression),
        ),
        string("udt_name", udt_name(data_type)),
        optional_i64("character_maximum_length", character_length(data_type)),
        optional_i64("numeric_precision", numeric_precision(data_type)),
        optional_i64("numeric_scale", numeric_scale(data_type)),
        optional_i64("datetime_precision", datetime_precision(data_type)),
    ]
}

fn constraint_for_field<'a>(
    constraints: &'a [FieldConstraint],
    field: &str,
) -> Option<&'a FieldConstraint> {
    constraints
        .iter()
        .find(|constraint| constraint.field.eq_ignore_ascii_case(field))
}

fn is_not_nullable(constraint: Option<&FieldConstraint>) -> bool {
    constraint
        .map(|constraint| constraint.not_null || constraint.primary_key)
        .unwrap_or(false)
}

fn udt_name(data_type: &DataType) -> String {
    match data_type {
        DataType::Null => "unknown".to_string(),
        DataType::SmallInt => "int2".to_string(),
        DataType::Int => "int4".to_string(),
        DataType::BigInt => "int8".to_string(),
        DataType::Float => "float8".to_string(),
        DataType::Boolean => "bool".to_string(),
        DataType::Text => "text".to_string(),
        DataType::Char { .. } => "bpchar".to_string(),
        DataType::Varchar { .. } => "varchar".to_string(),
        DataType::Uuid => "uuid".to_string(),
        DataType::Bytea => "bytea".to_string(),
        DataType::Date => "date".to_string(),
        DataType::Time => "time".to_string(),
        DataType::Timestamp => "timestamp".to_string(),
        DataType::Vector(_) => "vector".to_string(),
        DataType::Json => "json".to_string(),
        DataType::Array(inner) => format!("_{}", udt_name(inner)),
    }
}

fn character_length(data_type: &DataType) -> Option<i64> {
    match data_type {
        DataType::Char { length } => Some(i64::from(length.unwrap_or(1))),
        DataType::Varchar { length } => length.map(i64::from),
        _ => None,
    }
}

fn numeric_precision(data_type: &DataType) -> Option<i64> {
    match data_type {
        DataType::SmallInt => Some(16),
        DataType::Int => Some(32),
        DataType::BigInt => Some(64),
        DataType::Float => Some(53),
        _ => None,
    }
}

fn numeric_scale(data_type: &DataType) -> Option<i64> {
    match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => Some(0),
        _ => None,
    }
}

fn datetime_precision(data_type: &DataType) -> Option<i64> {
    match data_type {
        DataType::Time | DataType::Timestamp => Some(6),
        _ => None,
    }
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

fn bool_value(name: &str, value: bool) -> (String, Value) {
    (name.to_string(), Value::Bool(value))
}

fn optional_string(name: &str, value: Option<String>) -> (String, Value) {
    (
        name.to_string(),
        value.map(Value::String).unwrap_or(Value::Null),
    )
}

fn optional_i64(name: &str, value: Option<i64>) -> (String, Value) {
    (
        name.to_string(),
        value.map(Value::Int64).unwrap_or(Value::Null),
    )
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
