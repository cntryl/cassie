use crate::app::CassieSession;
use crate::catalog::{
    local_name, relation_belongs_to_database, relation_database_name, relation_schema_name,
    schema_belongs_to_database, Catalog, DatabaseMeta, FieldConstraint,
};
use crate::types::{DataType, Value};

pub type VirtualRow = Vec<(String, Value)>;

pub(crate) fn index_definition_by_oid(catalog: &Catalog, oid: i64) -> Option<String> {
    virtual_views_pg::index_definition_by_oid(catalog, oid)
}

pub(crate) fn index_definition_by_name(catalog: &Catalog, name: &str) -> Option<String> {
    virtual_views_pg::index_definition_by_name(catalog, name)
}

pub(crate) fn format_stored_expression(expression: &str) -> String {
    virtual_views_pg::format_stored_expression(expression)
}

#[path = "virtual_views_consistency.rs"]
mod virtual_views_consistency;
#[path = "virtual_views_constraints.rs"]
mod virtual_views_constraints;
#[path = "virtual_views_empty.rs"]
mod virtual_views_empty;
#[path = "virtual_views_pg.rs"]
mod virtual_views_pg;
#[path = "virtual_views_projections.rs"]
mod virtual_views_projections;
#[path = "virtual_views_relations.rs"]
mod virtual_views_relations;
#[path = "virtual_views_repair.rs"]
mod virtual_views_repair;
#[path = "virtual_views_sequences.rs"]
mod virtual_views_sequences;
#[path = "virtual_views_session_objects.rs"]
mod virtual_views_session_objects;
#[path = "virtual_views_storage.rs"]
mod virtual_views_storage;
#[path = "virtual_views_timezones.rs"]
mod virtual_views_timezones;

#[must_use]
pub fn schema(name: &str) -> Option<Vec<(String, DataType)>> {
    let name = normalize_name(name);
    information_schema_schema(name.as_str()).or_else(|| pg_catalog_schema(name.as_str()))
}

#[must_use]
pub fn rows(
    catalog: &Catalog,
    name: &str,
    session: Option<&CassieSession>,
) -> Option<Vec<VirtualRow>> {
    let name = normalize_name(name);
    if name == "pg_catalog.pg_settings" {
        return Some(pg_settings_rows(session));
    }
    if let Some(rows) = virtual_views_session_objects::rows(name.as_str(), session) {
        return Some(rows);
    }
    let current_database = session.and_then(CassieSession::current_database);
    information_schema_rows_for(catalog, name.as_str(), current_database)
        .or_else(|| pg_catalog_rows_for(catalog, name.as_str(), current_database))
}

fn information_schema_schema(name: &str) -> Option<Vec<(String, DataType)>> {
    match name {
        "information_schema.tables" => Some(vec![
            text("table_schema"),
            text("table_name"),
            text("table_type"),
        ]),
        "information_schema.columns" => Some(vec![
            text("table_schema"),
            text("table_name"),
            text("column_name"),
            text("data_type"),
            int("ordinal_position"),
            text("is_nullable"),
            text("column_default"),
            text("udt_catalog"),
            text("udt_schema"),
            text("udt_name"),
            int("character_maximum_length"),
            int("numeric_precision"),
            int("numeric_scale"),
            int("datetime_precision"),
            text("is_identity"),
            text("identity_generation"),
            text("is_generated"),
        ]),
        "information_schema.views" => Some(vec![
            text("table_schema"),
            text("table_name"),
            text("view_definition"),
        ]),
        "information_schema.schemata" => Some(vec![text("catalog_name"), text("schema_name")]),
        "information_schema.sequences" => {
            Some(virtual_views_sequences::information_schema_sequences_schema())
        }
        "information_schema.table_constraints" => {
            Some(virtual_views_constraints::table_constraints_schema())
        }
        "information_schema.key_column_usage" => {
            Some(virtual_views_constraints::key_column_usage_schema())
        }
        "information_schema.referential_constraints" => {
            Some(virtual_views_constraints::referential_constraints_schema())
        }
        _ => None,
    }
}

fn pg_catalog_schema(name: &str) -> Option<Vec<(String, DataType)>> {
    pg_catalog_core_schema(name)
        .or_else(|| virtual_views_empty::schema(name))
        .or_else(|| virtual_views_relations::schema(name))
        .or_else(|| virtual_views_projections::schema(name))
        .or_else(|| pg_catalog_runtime_schema(name))
        .or_else(|| virtual_views_timezones::schema(name))
        .or_else(|| virtual_views_session_objects::schema(name))
}

fn pg_catalog_core_schema(name: &str) -> Option<Vec<(String, DataType)>> {
    match name {
        "pg_catalog.pg_database" => Some(vec![
            int("oid"),
            text("datname"),
            int("datdba"),
            text("encoding"),
            text("datcollate"),
            text("datctype"),
        ]),
        "pg_catalog.pg_namespace" => Some(virtual_views_pg::namespace_schema()),
        "pg_catalog.pg_class" => Some(virtual_views_pg::class_schema()),
        "pg_catalog.pg_am" => Some(virtual_views_pg::access_method_schema()),
        "pg_catalog.pg_attribute" => Some(virtual_views_pg::attribute_schema()),
        "pg_catalog.pg_indexes" => Some(virtual_views_pg::indexes_schema()),
        "pg_catalog.pg_index" => Some(virtual_views_pg::index_schema()),
        "pg_catalog.pg_attrdef" => Some(virtual_views_pg::attrdef_schema()),
        "pg_catalog.pg_table_storage" => Some(virtual_views_storage::schema()),
        "pg_catalog.pg_constraint" => Some(virtual_views_constraints::pg_constraint_schema()),
        "pg_catalog.pg_roles" => Some(vec![text("rolname"), bool("rolcanlogin"), bool("rolsuper")]),
        "pg_catalog.pg_settings" => Some(vec![
            text("name"),
            text("setting"),
            text("unit"),
            text("category"),
            text("short_desc"),
            text("extra_desc"),
            text("context"),
            text("vartype"),
            text("source"),
            text("min_val"),
            text("max_val"),
            text("enumvals"),
            text("boot_val"),
            text("reset_val"),
            text("sourcefile"),
            int("sourceline"),
            bool("pending_restart"),
        ]),
        "pg_catalog.pg_rollups" => Some(vec![
            text("rollup_name"),
            text("source_collection"),
            text("output_collection"),
            text("state"),
            int("lag_rows"),
            text("bucket_expr"),
        ]),
        "pg_catalog.pg_maintenance_debt" => Some(vec![
            text("collection"),
            text("artifact"),
            int("target_generation"),
            int("retry_count"),
            text("last_error"),
            text("fallback_reason"),
        ]),
        "pg_catalog.pg_type" => Some(virtual_views_pg::type_schema()),
        _ => None,
    }
}

fn pg_settings_rows(session: Option<&CassieSession>) -> Vec<VirtualRow> {
    crate::app::all_session_settings()
        .iter()
        .map(|spec| {
            let setting = session.map_or_else(
                || spec.default.to_string(),
                |session| {
                    session
                        .setting(spec.name)
                        .unwrap_or_else(|_| spec.default.to_string())
                },
            );
            vec![
                string("name", spec.name),
                string("setting", setting),
                ("unit".to_string(), Value::Null),
                string("category", "Client Connection Defaults"),
                string("short_desc", "Cassie compatibility setting"),
                ("extra_desc".to_string(), Value::Null),
                string(
                    "context",
                    if spec.kind == crate::app::SettingKind::Mutable {
                        "user"
                    } else {
                        "internal"
                    },
                ),
                string("vartype", "string"),
                string("source", "default"),
                ("min_val".to_string(), Value::Null),
                ("max_val".to_string(), Value::Null),
                ("enumvals".to_string(), Value::Null),
                string("boot_val", spec.default),
                string("reset_val", spec.default),
                ("sourcefile".to_string(), Value::Null),
                ("sourceline".to_string(), Value::Null),
                bool_value("pending_restart", false),
            ]
        })
        .collect()
}

fn pg_catalog_runtime_schema(name: &str) -> Option<Vec<(String, DataType)>> {
    match name {
        "pg_catalog.pg_operational_assignments" => Some(vec![
            text("assignment_id"),
            text("node_id"),
            text("projection_id"),
            text("tenant"),
            text("partition_key"),
            int("generation"),
            text("state"),
            text("routing_hint"),
            int("updated_ms"),
        ]),
        "pg_catalog.pg_retention_policies" => Some(vec![
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
        ]),
        _ => None,
    }
}

fn information_schema_rows_for(
    catalog: &Catalog,
    name: &str,
    current_database: Option<&str>,
) -> Option<Vec<VirtualRow>> {
    match name {
        "information_schema.tables" => Some(information_schema_tables(catalog, current_database)),
        "information_schema.columns" => Some(information_schema_columns(catalog, current_database)),
        "information_schema.views" => Some(information_schema_views(catalog, current_database)),
        "information_schema.schemata" => {
            Some(information_schema_schemata(catalog, current_database))
        }
        "information_schema.sequences" => Some(
            virtual_views_sequences::information_schema_sequences(catalog, current_database),
        ),
        "information_schema.table_constraints" => Some(
            virtual_views_constraints::table_constraints(catalog, current_database),
        ),
        "information_schema.key_column_usage" => Some(virtual_views_constraints::key_column_usage(
            catalog,
            current_database,
        )),
        "information_schema.referential_constraints" => Some(
            virtual_views_constraints::referential_constraints(catalog, current_database),
        ),
        _ => None,
    }
}

fn pg_catalog_rows_for(
    catalog: &Catalog,
    name: &str,
    current_database: Option<&str>,
) -> Option<Vec<VirtualRow>> {
    let rows = pg_catalog_core_rows(catalog, name, current_database)
        .or_else(|| virtual_views_projections::rows(catalog, name))
        .or_else(|| pg_catalog_runtime_rows(catalog, name))
        .or_else(|| virtual_views_timezones::rows(name))?;
    Some(filter_pg_catalog_rows(
        catalog,
        name,
        current_database,
        rows,
    ))
}

fn filter_pg_catalog_rows(
    catalog: &Catalog,
    name: &str,
    current_database: Option<&str>,
    rows: Vec<VirtualRow>,
) -> Vec<VirtualRow> {
    let Some(database) = current_database else {
        return rows;
    };
    let projection_ids = if matches!(
        name,
        "pg_catalog.pg_projection_consistency_reports" | "pg_catalog.pg_operational_assignments"
    ) {
        database_scoped_projection_ids(catalog, database)
    } else {
        std::collections::HashSet::new()
    };

    rows.into_iter()
        .filter(|row| match name {
            "pg_catalog.pg_rollups" => row_relation_belongs_to_database(row, "source_collection", database),
            "pg_catalog.pg_maintenance_debt"
            | "pg_catalog.pg_retention_policies"
            | "pg_catalog.pg_projection_checkpoints" => {
                row_relation_belongs_to_database(row, "collection", database)
            }
            "pg_catalog.pg_materialized_projections"
            | "pg_catalog.pg_projection_versions"
            | "pg_catalog.pg_projection_operations"
            | "pg_catalog.pg_projection_hashes"
            | "pg_catalog.pg_projection_integrity_reports"
            | "pg_catalog.pg_projection_repair_reports" => {
                row_relation_belongs_to_database(row, "projection_name", database)
            }
            "pg_catalog.pg_projection_comparison_reports" => {
                row_relation_belongs_to_database(row, "target", database)
            }
            "pg_catalog.pg_projection_consistency_reports"
            | "pg_catalog.pg_operational_assignments" => row
                .iter()
                .find(|(column, _)| column == "projection_id")
                .is_some_and(|(_, value)| {
                    matches!(value, Value::String(projection_id) if projection_ids.contains(&projection_id.to_ascii_lowercase()))
                }),
            _ => true,
        })
        .collect()
}

fn database_scoped_projection_ids(
    catalog: &Catalog,
    database: &str,
) -> std::collections::HashSet<String> {
    let mut owners_by_id =
        std::collections::HashMap::<String, std::collections::HashSet<String>>::new();
    for collection in catalog.list_collections_canonical() {
        if let Some(owner) = relation_database_name(&collection.name) {
            add_projection_id_owner(&mut owners_by_id, &collection.name, &owner);
        }
    }
    for projection in catalog.list_projection_metadata() {
        if let Some(owner) = relation_database_name(&projection.collection) {
            add_projection_id_owner(&mut owners_by_id, &projection.collection, &owner);
            add_projection_id_owner(&mut owners_by_id, projection.projection_id(), &owner);
        }
    }
    let database = database.to_ascii_lowercase();
    owners_by_id
        .into_iter()
        .filter_map(|(projection_id, owners)| {
            (owners.len() == 1 && owners.contains(&database)).then_some(projection_id)
        })
        .collect()
}

fn add_projection_id_owner(
    owners_by_id: &mut std::collections::HashMap<String, std::collections::HashSet<String>>,
    projection_id: &str,
    owner: &str,
) {
    let owner = owner.to_ascii_lowercase();
    owners_by_id
        .entry(projection_id.to_ascii_lowercase())
        .or_default()
        .insert(owner.clone());
    owners_by_id
        .entry(local_name(projection_id).to_ascii_lowercase())
        .or_default()
        .insert(owner);
}

fn row_relation_belongs_to_database(row: &VirtualRow, column: &str, database: &str) -> bool {
    row.iter()
        .find(|(name, _)| name == column)
        .is_some_and(|(_, value)| {
            matches!(value, Value::String(relation) if relation_belongs_to_database(relation, database))
        })
}

fn pg_catalog_core_rows(
    catalog: &Catalog,
    name: &str,
    current_database: Option<&str>,
) -> Option<Vec<VirtualRow>> {
    match name {
        "pg_catalog.pg_database" => Some(pg_database_rows(catalog)),
        "pg_catalog.pg_namespace" => {
            Some(virtual_views_pg::pg_namespace(catalog, current_database))
        }
        "pg_catalog.pg_class" => Some(virtual_views_pg::pg_class(catalog, current_database)),
        "pg_catalog.pg_am" => Some(virtual_views_pg::pg_am()),
        "pg_catalog.pg_attribute" => {
            Some(virtual_views_pg::pg_attribute(catalog, current_database))
        }
        "pg_catalog.pg_indexes" => Some(virtual_views_pg::pg_indexes(catalog, current_database)),
        "pg_catalog.pg_index" => Some(virtual_views_pg::pg_index(catalog, current_database)),
        "pg_catalog.pg_attrdef" => Some(virtual_views_pg::pg_attrdef(catalog, current_database)),
        "pg_catalog.pg_table_storage" => {
            Some(virtual_views_storage::rows(catalog, current_database))
        }
        "pg_catalog.pg_constraint" => Some(virtual_views_constraints::pg_constraint(
            catalog,
            current_database,
        )),
        "pg_catalog.pg_roles" => Some(pg_roles_rows(catalog)),
        "pg_catalog.pg_rollups" => Some(pg_rollups_rows(catalog)),
        "pg_catalog.pg_maintenance_debt" => Some(pg_maintenance_debt_rows(catalog)),
        "pg_catalog.pg_type" => Some(virtual_views_pg::pg_type(catalog, current_database)),
        _ => virtual_views_empty::rows(name)
            .or_else(|| virtual_views_relations::rows(catalog, name, current_database)),
    }
}

fn pg_catalog_runtime_rows(catalog: &Catalog, name: &str) -> Option<Vec<VirtualRow>> {
    match name {
        "pg_catalog.pg_operational_assignments" => Some(pg_operational_assignments_rows(catalog)),
        "pg_catalog.pg_retention_policies" => Some(pg_retention_policies_rows(catalog)),
        _ => None,
    }
}

fn pg_roles_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_roles()
        .into_iter()
        .map(|role| {
            vec![
                string("rolname", role.name),
                bool_value("rolcanlogin", role.can_login),
                bool_value("rolsuper", role.is_admin),
            ]
        })
        .collect()
}

fn pg_rollups_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_rollups()
        .into_iter()
        .map(|rollup| {
            vec![
                string("rollup_name", rollup.name),
                string("source_collection", rollup.source_collection),
                string("output_collection", rollup.output_collection),
                string("state", rollup.state.as_str()),
                int_value("lag_rows", rollup.refresh_cursor.lag_rows),
                string("bucket_expr", rollup.bucket_expr),
            ]
        })
        .collect()
}

fn pg_maintenance_debt_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_maintenance_debts()
        .into_iter()
        .map(|debt| {
            vec![
                string("collection", debt.collection),
                string("artifact", debt.artifact),
                int_value("target_generation", debt.target_generation),
                int_value("retry_count", debt.retry_count),
                string("last_error", debt.last_error.unwrap_or_default()),
                string("fallback_reason", debt.fallback_reason),
            ]
        })
        .collect()
}

fn pg_operational_assignments_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
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
                int_value("generation", assignment.generation),
                string("state", assignment.state.as_str()),
                string("routing_hint", assignment.routing_hint.unwrap_or_default()),
                int_value("updated_ms", assignment.updated_ms),
            ]
        })
        .collect()
}

fn pg_retention_policies_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
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
                    policy.last_enforced_ms.unwrap_or_default(),
                ),
                int_value("last_deleted_rows", policy.last_deleted_rows),
                int_value("last_skipped_rows", policy.last_skipped_rows),
                string("last_error", policy.last_error.unwrap_or_default()),
            ]
        })
        .collect()
}

fn normalize_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn pg_database_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_databases()
        .into_iter()
        .map(database_row)
        .collect::<Vec<_>>();
    rows.sort_by_key(row_sort_key);
    rows
}

fn database_row(database: DatabaseMeta) -> VirtualRow {
    vec![
        int_value("oid", virtual_views_pg::database_oid(&database.name)),
        string("datname", database.name),
        int_value("datdba", virtual_views_pg::postgres_role_oid()),
        string("encoding", "UTF8"),
        string("datcollate", "C"),
        string("datctype", "C"),
    ]
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

fn information_schema_tables(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_collections_canonical()
        .into_iter()
        .filter(|collection| {
            current_database
                .is_none_or(|database| relation_belongs_to_database(&collection.name, database))
        })
        .map(|collection| {
            vec![
                string("table_schema", relation_schema_name(&collection.name)),
                string("table_name", local_name(&collection.name)),
                string("table_type", "BASE TABLE"),
            ]
        })
        .collect::<Vec<_>>();

    rows.extend(
        catalog
            .list_views()
            .into_iter()
            .filter(|view| {
                current_database
                    .is_none_or(|database| relation_belongs_to_database(&view.name, database))
            })
            .map(|view| {
                vec![
                    string("table_schema", relation_schema_name(&view.name)),
                    string("table_name", local_name(&view.name)),
                    string("table_type", "VIEW"),
                ]
            }),
    );

    rows.sort_by_key(row_sort_key);
    rows
}

fn information_schema_columns(
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
                current_database,
            ));
        }
    }
    for view in catalog.list_views() {
        if current_database
            .is_some_and(|database| !relation_belongs_to_database(&view.name, database))
        {
            continue;
        }
        for (index, field) in view.schema.fields.iter().enumerate() {
            rows.push(information_schema_column_row(
                &view.name,
                &field.name,
                &field.data_type,
                index,
                None,
                current_database,
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
    current_database: Option<&str>,
) -> VirtualRow {
    vec![
        string("table_schema", relation_schema_name(relation)),
        string("table_name", local_name(relation)),
        string("column_name", field_name),
        string("data_type", data_type_name(data_type)),
        int_value("ordinal_position", index + 1),
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
            // Identity columns report their generation, not a default.
            constraint
                .filter(|constraint| constraint.identity.is_none())
                .and_then(virtual_views_pg::constraint_default_expression),
        ),
        string(
            "udt_catalog",
            relation_database_name(relation)
                .unwrap_or_else(|| current_database.unwrap_or("postgres").to_owned()),
        ),
        string("udt_schema", crate::catalog::PG_CATALOG_SCHEMA),
        string("udt_name", udt_name(data_type)),
        optional_i64("character_maximum_length", character_length(data_type)),
        optional_i64("numeric_precision", numeric_precision(data_type)),
        optional_i64("numeric_scale", numeric_scale(data_type)),
        optional_i64("datetime_precision", datetime_precision(data_type)),
        string(
            "is_identity",
            if constraint
                .and_then(|constraint| constraint.identity)
                .is_some()
            {
                "YES"
            } else {
                "NO"
            },
        ),
        optional_string(
            "identity_generation",
            constraint
                .and_then(|constraint| constraint.identity)
                .map(|generation| generation.label().to_string()),
        ),
        string("is_generated", "NEVER"),
    ]
}

fn constraint_for_field<'a>(
    constraints: &'a [FieldConstraint],
    field: &str,
) -> Option<&'a FieldConstraint> {
    constraints.iter().find(|constraint| {
        crate::sql::ColumnIdentifierPath::stored_field_key(&constraint.field)
            == crate::sql::ColumnIdentifierPath::stored_field_key(field)
    })
}

fn is_not_nullable(constraint: Option<&FieldConstraint>) -> bool {
    constraint.is_some_and(|constraint| constraint.not_null || constraint.primary_key)
}

fn udt_name(data_type: &DataType) -> String {
    use crate::types::builtin::{type_metadata, TypeMetadata};

    match type_metadata(data_type) {
        TypeMetadata::Scalar(metadata) => metadata.name.to_owned(),
        TypeMetadata::Vector(_) => "vector".to_owned(),
        TypeMetadata::Array(inner) => format!("_{}", udt_name(inner)),
    }
}

fn character_length(data_type: &DataType) -> Option<i64> {
    match data_type {
        DataType::Char { length } => Some(i64::from(length.unwrap_or(1))),
        DataType::Varchar { length }
        | DataType::Extended(
            crate::types::extended::ExtendedType::Bit { length }
            | crate::types::extended::ExtendedType::VarBit { length },
        ) => length.map(i64::from),
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

fn information_schema_views(catalog: &Catalog, current_database: Option<&str>) -> Vec<VirtualRow> {
    let mut rows = catalog
        .list_views()
        .into_iter()
        .filter(|view| {
            current_database
                .is_none_or(|database| relation_belongs_to_database(&view.name, database))
        })
        .map(|view| {
            vec![
                string("table_schema", relation_schema_name(&view.name)),
                string("table_name", local_name(&view.name)),
                string("view_definition", view.query),
            ]
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(row_sort_key);
    rows
}

fn information_schema_schemata(
    catalog: &Catalog,
    current_database: Option<&str>,
) -> Vec<VirtualRow> {
    let database = current_database.unwrap_or("postgres");
    let mut rows = vec![
        vec![
            string("catalog_name", database),
            string("schema_name", "information_schema"),
        ],
        vec![
            string("catalog_name", database),
            string("schema_name", "pg_catalog"),
        ],
        vec![
            string("catalog_name", database),
            string("schema_name", "public"),
        ],
    ];
    rows.extend(
        catalog
            .list_namespaces()
            .into_iter()
            .filter(|namespace| {
                current_database.is_none_or(|db| schema_belongs_to_database(&namespace.name, db))
            })
            .map(|namespace| {
                vec![
                    string("catalog_name", database),
                    string("schema_name", local_name(&namespace.name)),
                ]
            }),
    );
    rows.sort_by_key(row_sort_key);
    rows.dedup_by_key(|row| row_sort_key(row));
    rows
}

fn data_type_name(data_type: &DataType) -> String {
    match data_type {
        DataType::Null => "null".to_string(),
        DataType::SmallInt => "smallint".to_string(),
        DataType::Int => "integer".to_string(),
        DataType::BigInt => "bigint".to_string(),
        DataType::Float => "double precision".to_string(),
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
        DataType::ObjectId(kind) => kind.name().to_string(),
        DataType::Extended(kind) => kind.name().to_string(),
    }
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

fn optional_string(name: &str, value: Option<String>) -> (String, Value) {
    (name.to_string(), value.map_or(Value::Null, Value::String))
}

fn optional_i64(name: &str, value: Option<i64>) -> (String, Value) {
    (name.to_string(), value.map_or(Value::Null, Value::Int64))
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
            Value::Typed(value) => value.text(),
        })
        .collect::<Vec<_>>()
        .join("|")
}
