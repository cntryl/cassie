use cassie::catalog::{
    canonical_relation_name, FunctionArgMeta, FunctionMeta, ProcedureMeta, Volatility,
};
use cassie::types::{DataType, FieldSchema, Schema};

#[test]
fn should_round_trip_one_database_as_bounded_chunks() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    cassie
        .midge
        .create_database("analytics", None)
        .expect("database");
    cassie
        .midge
        .create_namespace("analytics.public")
        .expect("namespace");
    let source_collection = canonical_relation_name("analytics", "public", "docs");
    cassie
        .midge
        .create_collection(
            &source_collection,
            Schema {
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                }],
            },
        )
        .expect("collection");
    cassie
        .midge
        .put_document(
            &source_collection,
            Some("row-1".to_string()),
            serde_json::json!({"value": "from-image"}),
        )
        .expect("row");

    let mut backup = cassie
        .begin_database_backup("analytics")
        .expect("begin backup");
    let mut image = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("backup chunk") {
        assert!(chunk.len() <= 64 * 1024);
        image.extend_from_slice(&chunk);
    }

    // Act
    let mut restore = cassie
        .begin_database_restore("restored")
        .expect("begin restore");
    for chunk in image.chunks(3) {
        restore.push_chunk(chunk).expect("restore chunk");
    }
    restore.finish().expect("finish restore");
    cassie.hydrate_catalog().expect("hydrate restored catalog");

    // Assert
    let target_collection = canonical_relation_name("restored", "public", "docs");
    let row = cassie
        .midge
        .get_document(&target_collection, "row-1")
        .expect("restored row lookup")
        .expect("restored row");
    assert_eq!(row.payload["value"], "from-image");
    assert!(cassie
        .midge
        .get_document(&source_collection, "row-1")
        .expect("source row lookup")
        .is_some());
}

#[test]
fn should_reject_qualified_database_restore_targets() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");

    // Act
    let result = cassie.begin_database_restore("tenant.db");

    // Assert
    assert!(result.is_err(), "qualified database targets are invalid");
}

#[test]
fn should_reject_restore_when_target_matches_existing_database_case_insensitively() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    cassie
        .midge
        .create_database("analytics", None)
        .expect("analytics");
    cassie
        .midge
        .create_namespace("analytics.public")
        .expect("analytics namespace");
    let collection = canonical_relation_name("analytics", "public", "parent");
    cassie
        .midge
        .create_collection(
            &collection,
            Schema {
                fields: vec![
                    FieldSchema {
                        name: "id".to_string(),
                        data_type: DataType::Int,
                        nullable: false,
                    },
                    FieldSchema {
                        name: "code".to_string(),
                        data_type: DataType::Text,
                        nullable: false,
                    },
                ],
            },
        )
        .expect("create parent collection");
    cassie
        .midge
        .put_document(
            &collection,
            Some("1".to_string()),
            serde_json::json!({"id": 1, "code": "live"}),
        )
        .expect("insert parent row");

    // Act
    let result = cassie.begin_database_restore("ANALYTICS");

    // Assert
    assert!(result.is_err(), "restore must not replace a live database");
    let live_row = cassie
        .midge
        .get_document("analytics.public.parent", "1")
        .expect("live row lookup")
        .expect("live row remains present");
    assert_eq!(live_row.payload["code"], "live");
}

#[test]
fn should_round_trip_program_catalog_metadata() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    cassie
        .midge
        .create_database("analytics", None)
        .expect("create source database");
    cassie
        .midge
        .create_namespace("analytics.public")
        .expect("create source namespace");
    cassie
        .midge
        .put_function(&FunctionMeta {
            name: canonical_relation_name("analytics", "public", "marker_function"),
            args: vec![FunctionArgMeta {
                name: "x".to_string(),
                data_type: DataType::Int,
            }],
            return_type: DataType::Int,
            volatility: Volatility::Immutable,
            body: "x * 2".to_string(),
        })
        .expect("store source function");
    cassie
        .midge
        .put_procedure(&ProcedureMeta {
            name: canonical_relation_name("analytics", "public", "marker_procedure"),
            args: vec![],
            body: "SELECT 1".to_string(),
        })
        .expect("store source procedure");

    let mut backup = cassie
        .begin_database_backup("analytics")
        .expect("begin backup");
    let mut image = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("backup chunk") {
        image.extend_from_slice(&chunk);
    }

    // Act
    let mut restore = cassie
        .begin_database_restore("restored")
        .expect("begin restore");
    for chunk in image.chunks(3) {
        restore.push_chunk(chunk).expect("restore chunk");
    }
    restore.finish().expect("finish restore");
    cassie.hydrate_catalog().expect("hydrate restored catalog");
    let restored_function = cassie
        .midge
        .get_function("restored.public.marker_function")
        .expect("read restored function");
    let restored_procedure = cassie
        .midge
        .get_procedure("restored.public.marker_procedure")
        .expect("read restored procedure");

    // Assert
    assert_eq!(
        restored_function,
        Some(FunctionMeta {
            name: canonical_relation_name("restored", "public", "marker_function"),
            args: vec![FunctionArgMeta {
                name: "x".to_string(),
                data_type: DataType::Int,
            }],
            return_type: DataType::Int,
            volatility: Volatility::Immutable,
            body: "x * 2".to_string(),
        })
    );
    assert_eq!(
        restored_procedure,
        Some(ProcedureMeta {
            name: canonical_relation_name("restored", "public", "marker_procedure"),
            args: vec![],
            body: "SELECT 1".to_string(),
        })
    );
}
