use crate::types::DataType;

#[test]
fn should_preserve_virtual_schema_dispatch_with_normalized_existence() {
    // Arrange
    let cases = [
        ("information_schema.schemata", "catalog_name"),
        ("pg_catalog.pg_roles", "rolname"),
        ("pg_catalog.pg_auth_members", "oid"),
        ("pg_catalog.pg_tables", "schemaname"),
        ("pg_catalog.pg_projection_checkpoints", "projection_id"),
        ("pg_catalog.pg_operational_assignments", "assignment_id"),
        ("pg_catalog.pg_timezone_names", "name"),
        ("pg_catalog.pg_cursors", "name"),
    ];
    // Act / Assert
    for (name, first) in cases {
        let schema = super::schema(name).expect("virtual schema");
        // Assert
        assert_eq!(
            schema.first().map(|field| field.0.as_str()),
            Some(first),
            "{name}"
        );
        assert!(super::schema_exists(name), "{name}");
        assert!(super::schema_exists(&format!(
            "\u{2003}{}\u{2003}",
            name.to_ascii_uppercase()
        )));
        assert_eq!(
            super::schema(&format!("  {}  ", name.to_ascii_uppercase())),
            Some(schema)
        );
    }
    for name in [
        "",
        "pg_catalog.pg_missing",
        "information_schema.schemata.extra",
    ] {
        assert!(!super::schema_exists(name));
        assert!(super::schema(name).is_none());
    }
}

#[test]
fn should_preserve_known_virtual_schema_column_types() {
    // Arrange
    let expected = vec![
        ("catalog_name".to_owned(), DataType::Text),
        ("schema_name".to_owned(), DataType::Text),
    ];
    // Act
    let actual = super::schema("information_schema.schemata");
    // Assert
    assert_eq!(actual, Some(expected));
}

#[test]
fn should_retain_every_existing_virtual_schema_name() {
    // Arrange
    let names = [
        "information_schema.columns",
        "information_schema.key_column_usage",
        "information_schema.referential_constraints",
        "information_schema.schemata",
        "information_schema.sequences",
        "information_schema.table_constraints",
        "information_schema.tables",
        "information_schema.views",
        "pg_catalog.pg_am",
        "pg_catalog.pg_attrdef",
        "pg_catalog.pg_attribute",
        "pg_catalog.pg_auth_members",
        "pg_catalog.pg_class",
        "pg_catalog.pg_collation",
        "pg_catalog.pg_constraint",
        "pg_catalog.pg_cursors",
        "pg_catalog.pg_database",
        "pg_catalog.pg_db_role_setting",
        "pg_catalog.pg_default_acl",
        "pg_catalog.pg_depend",
        "pg_catalog.pg_enum",
        "pg_catalog.pg_event_trigger",
        "pg_catalog.pg_foreign_data_wrapper",
        "pg_catalog.pg_foreign_server",
        "pg_catalog.pg_foreign_table",
        "pg_catalog.pg_index",
        "pg_catalog.pg_indexes",
        "pg_catalog.pg_init_privs",
        "pg_catalog.pg_largeobject",
        "pg_catalog.pg_largeobject_metadata",
        "pg_catalog.pg_maintenance_debt",
        "pg_catalog.pg_materialized_projections",
        "pg_catalog.pg_matviews",
        "pg_catalog.pg_namespace",
        "pg_catalog.pg_operational_assignments",
        "pg_catalog.pg_parameter_acl",
        "pg_catalog.pg_partitioned_table",
        "pg_catalog.pg_policy",
        "pg_catalog.pg_prepared_statements",
        "pg_catalog.pg_proc",
        "pg_catalog.pg_projection_checkpoints",
        "pg_catalog.pg_projection_comparison_reports",
        "pg_catalog.pg_projection_consistency_reports",
        "pg_catalog.pg_projection_hashes",
        "pg_catalog.pg_projection_integrity_reports",
        "pg_catalog.pg_projection_operations",
        "pg_catalog.pg_projection_repair_reports",
        "pg_catalog.pg_projection_versions",
        "pg_catalog.pg_publication",
        "pg_catalog.pg_publication_namespace",
        "pg_catalog.pg_publication_rel",
        "pg_catalog.pg_replication_origin",
        "pg_catalog.pg_retention_policies",
        "pg_catalog.pg_roles",
        "pg_catalog.pg_rollups",
        "pg_catalog.pg_seclabel",
        "pg_catalog.pg_sequence",
        "pg_catalog.pg_sequences",
        "pg_catalog.pg_settings",
        "pg_catalog.pg_shdepend",
        "pg_catalog.pg_shdescription",
        "pg_catalog.pg_shseclabel",
        "pg_catalog.pg_subscription",
        "pg_catalog.pg_subscription_rel",
        "pg_catalog.pg_table_storage",
        "pg_catalog.pg_tables",
        "pg_catalog.pg_timezone_names",
        "pg_catalog.pg_transform",
        "pg_catalog.pg_trigger",
        "pg_catalog.pg_type",
        "pg_catalog.pg_user_mapping",
        "pg_catalog.pg_views",
    ];
    // Act / Assert
    for name in names {
        // Assert
        assert!(super::schema_exists(name), "{name}");
        assert!(super::schema(name).is_some(), "{name}");
    }
}
