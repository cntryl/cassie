//! Lazy descriptors for the authoritative virtual-schema dispatchers.
use super::{
    bool, int, text, virtual_views_constraints, virtual_views_dependencies, virtual_views_empty,
    virtual_views_extensions, virtual_views_pg, virtual_views_procedures,
    virtual_views_projections, virtual_views_relations, virtual_views_sequences,
    virtual_views_session_objects, virtual_views_shared_dependencies, virtual_views_storage,
    virtual_views_timezones, DataType, INFORMATION_SCHEMA_TABLES,
};

pub(super) type SchemaFactory = fn() -> Vec<(String, DataType)>;

pub(super) enum Descriptor {
    Factory(SchemaFactory),
    Empty(virtual_views_empty::Fields),
}

impl Descriptor {
    pub(super) fn build(self) -> Vec<(String, DataType)> {
        match self {
            Self::Factory(factory) => factory(),
            Self::Empty(fields) => virtual_views_empty::build_fields(fields),
        }
    }
}

pub(super) fn lookup(name: &str) -> Option<Descriptor> {
    information_schema_schema(name)
        .map(Descriptor::Factory)
        .or_else(|| pg_catalog_core_schema(name).map(Descriptor::Factory))
        .or_else(|| virtual_views_empty::fields(name).map(Descriptor::Empty))
        .or_else(|| virtual_views_relations::schema_factory(name).map(Descriptor::Factory))
        .or_else(|| virtual_views_projections::schema_factory(name).map(Descriptor::Factory))
        .or_else(|| pg_catalog_runtime_schema(name).map(Descriptor::Factory))
        .or_else(|| virtual_views_timezones::schema_factory(name).map(Descriptor::Factory))
        .or_else(|| virtual_views_session_objects::schema_factory(name).map(Descriptor::Factory))
}

fn information_schema_schema(name: &str) -> Option<SchemaFactory> {
    match name {
        "information_schema.tables" => Some(|| {
            INFORMATION_SCHEMA_TABLES
                .iter()
                .map(|(field, data_type)| ((*field).to_owned(), data_type.clone()))
                .collect()
        }),
        "information_schema.columns" => Some(|| {
            vec![
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
            ]
        }),
        "information_schema.views" => Some(|| {
            vec![
                text("table_schema"),
                text("table_name"),
                text("view_definition"),
            ]
        }),
        "information_schema.schemata" => Some(|| vec![text("catalog_name"), text("schema_name")]),
        "information_schema.sequences" => {
            Some(virtual_views_sequences::information_schema_sequences_schema)
        }
        "information_schema.table_constraints" => {
            Some(virtual_views_constraints::table_constraints_schema)
        }
        "information_schema.key_column_usage" => {
            Some(virtual_views_constraints::key_column_usage_schema)
        }
        "information_schema.referential_constraints" => {
            Some(virtual_views_constraints::referential_constraints_schema)
        }
        _ => None,
    }
}

fn pg_catalog_core_schema(name: &str) -> Option<SchemaFactory> {
    match name {
        "pg_catalog.pg_database" => Some(|| {
            vec![
                int("oid"),
                text("datname"),
                int("datdba"),
                text("encoding"),
                text("datcollate"),
                text("datctype"),
            ]
        }),
        "pg_catalog.pg_extension" => Some(virtual_views_extensions::installed_schema),
        "pg_catalog.pg_available_extensions" => Some(virtual_views_extensions::available_schema),
        "pg_catalog.pg_namespace" => Some(virtual_views_pg::namespace_schema),
        "pg_catalog.pg_class" => Some(virtual_views_pg::class_schema),
        "pg_catalog.pg_am" => Some(virtual_views_pg::access_method_schema),
        "pg_catalog.pg_operator" => {
            Some(virtual_views_pg::vector_catalogs::operator_schema)
        }
        "pg_catalog.pg_opfamily" => {
            Some(virtual_views_pg::vector_catalogs::operator_family_schema)
        }
        "pg_catalog.pg_opclass" => {
            Some(virtual_views_pg::vector_catalogs::operator_class_schema)
        }
        "pg_catalog.pg_amop" => Some(
            virtual_views_pg::vector_catalogs::access_method_operator_schema,
        ),
        "pg_catalog.pg_collation" => Some(virtual_views_pg::collation::schema),
        "pg_catalog.pg_description" => Some(virtual_views_pg::descriptions::schema),
        "pg_catalog.pg_attribute" => Some(virtual_views_pg::attribute_schema),
        "pg_catalog.pg_indexes" => Some(virtual_views_pg::indexes_schema),
        "pg_catalog.pg_index" => Some(virtual_views_pg::index_schema),
        "pg_catalog.pg_attrdef" => Some(virtual_views_pg::attrdef_schema),
        "pg_catalog.pg_table_storage" => Some(virtual_views_storage::schema),
        "pg_catalog.pg_constraint" => Some(virtual_views_constraints::pg_constraint_schema),
        "pg_catalog.pg_depend" => Some(virtual_views_dependencies::schema),
        "pg_catalog.pg_shdepend" => Some(virtual_views_shared_dependencies::schema),
        "pg_catalog.pg_roles" => {
            Some(|| vec![text("rolname"), bool("rolcanlogin"), bool("rolsuper")])
        }
        "pg_catalog.pg_settings" => Some(|| {
            vec![
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
            ]
        }),
        "pg_catalog.pg_rollups" => Some(|| {
            vec![
                text("rollup_name"),
                text("source_collection"),
                text("output_collection"),
                text("state"),
                int("lag_rows"),
                text("bucket_expr"),
            ]
        }),
        "pg_catalog.pg_maintenance_debt" => Some(|| {
            vec![
                text("collection"),
                text("artifact"),
                int("target_generation"),
                int("retry_count"),
                text("last_error"),
                text("fallback_reason"),
            ]
        }),
        "pg_catalog.pg_type" => Some(virtual_views_pg::type_schema),
        "pg_catalog.pg_proc" => Some(virtual_views_procedures::schema),
        _ => None,
    }
}

fn pg_catalog_runtime_schema(name: &str) -> Option<SchemaFactory> {
    match name {
        "pg_catalog.pg_operational_assignments" => Some(|| {
            vec![
                text("assignment_id"),
                text("node_id"),
                text("projection_id"),
                text("tenant"),
                text("partition_key"),
                int("generation"),
                text("state"),
                text("routing_hint"),
                int("updated_ms"),
            ]
        }),
        "pg_catalog.pg_retention_policies" => Some(|| {
            vec![
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
            ]
        }),
        _ => None,
    }
}
