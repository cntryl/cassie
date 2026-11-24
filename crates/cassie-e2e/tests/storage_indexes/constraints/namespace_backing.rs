//! Namespace/backing probes for #953; ordinary SQL fallback remains authoritative.
use crate::support_catalog_directory::CatalogDirectory;
use crate::support_constraint_namespace::{
    fixture, local_unpublished_fixture, unpublished_fixture, CANONICAL,
};
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;

#[test]
fn should_resolve_constraint_metadata_given_unqualified_publication_when_writes_use_canonical_names(
) {
    // Arrange
    let (cassie, expected) = fixture("namespace_probe");
    // Act
    let actual = cassie.midge.load_constraints(CANONICAL).unwrap();
    // Assert
    assert_eq!(
        actual,
        vec![expected],
        "metadata must describe the canonical write target"
    );
}

#[test]
fn should_enforce_direct_unique_writes_given_unqualified_publication_when_committed_values_match() {
    // Arrange
    let (cassie, _) = fixture("namespace_probe");
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("first".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    let result = cassie.midge.put_document(
        CANONICAL,
        Some("second".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&result, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "declared UNIQUE must reject the duplicate at direct storage ingress: {result:?}"
    );
}

#[test]
fn should_resolve_constraint_metadata_given_schema_qualified_publication_when_writes_use_canonical_names(
) {
    // Arrange
    let (cassie, expected) = fixture("public.namespace_probe");
    // Act
    let actual = cassie.midge.load_constraints(CANONICAL).unwrap();
    // Assert
    assert_eq!(
        actual,
        vec![expected],
        "metadata must describe the canonical write target"
    );
}

#[test]
fn should_enforce_direct_unique_writes_given_schema_qualified_publication_when_committed_values_match(
) {
    // Arrange
    let (cassie, _) = fixture("public.namespace_probe");
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("first".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    let result = cassie.midge.put_document(
        CANONICAL,
        Some("second".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&result, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "declared UNIQUE must reject the duplicate at direct storage ingress: {result:?}"
    );
}

#[test]
fn should_resolve_constraint_metadata_given_database_qualified_publication_when_writes_use_canonical_names(
) {
    // Arrange
    let (cassie, expected) = fixture("postgres.public.namespace_probe");
    // Act
    let actual = cassie.midge.load_constraints(CANONICAL).unwrap();
    // Assert
    assert_eq!(
        actual,
        vec![expected],
        "metadata must describe the canonical write target"
    );
}

#[test]
fn should_enforce_direct_unique_writes_given_database_qualified_publication_when_committed_values_match(
) {
    // Arrange
    let (cassie, _) = fixture("postgres.public.namespace_probe");
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("first".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    let result = cassie.midge.put_document(
        CANONICAL,
        Some("second".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&result, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "declared UNIQUE must reject the duplicate at direct storage ingress: {result:?}"
    );
}

#[test]
fn should_backfill_existing_owners_given_unqualified_publication_when_new_writes_use_canonical_names(
) {
    // Arrange
    let (cassie, constraint) = unpublished_fixture();
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("existing".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints_with_unique_reservations(
            "namespace_probe",
            std::slice::from_ref(&constraint),
        )
        .unwrap();
    let result = cassie.midge.put_document(
        CANONICAL,
        Some("new".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&result, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "backfilled committed owner must reject the duplicate: {result:?}"
    );
}

#[test]
fn should_backfill_existing_owners_given_schema_qualified_publication_when_new_writes_use_canonical_names(
) {
    // Arrange
    let (cassie, constraint) = unpublished_fixture();
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("existing".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints_with_unique_reservations(
            "public.namespace_probe",
            std::slice::from_ref(&constraint),
        )
        .unwrap();
    let result = cassie.midge.put_document(
        CANONICAL,
        Some("new".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&result, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "backfilled committed owner must reject the duplicate: {result:?}"
    );
}

#[test]
fn should_backfill_existing_owners_given_database_qualified_publication_when_new_writes_use_canonical_names(
) {
    // Arrange
    let (cassie, constraint) = unpublished_fixture();
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("existing".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints_with_unique_reservations(
            "postgres.public.namespace_probe",
            std::slice::from_ref(&constraint),
        )
        .unwrap();
    let result = cassie.midge.put_document(
        CANONICAL,
        Some("new".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&result, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "backfilled committed owner must reject the duplicate: {result:?}"
    );
}

#[test]
fn should_preserve_unique_backing_given_unqualified_metadata_when_local_storage_restarts() {
    // Arrange
    let directory = CatalogDirectory::new("constraint-namespace-unqualified");
    let (cassie, expected) = local_unpublished_fixture(&directory.0);
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("persisted".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    cassie
        .midge
        .save_constraints_with_unique_reservations(
            "namespace_probe",
            std::slice::from_ref(&expected),
        )
        .unwrap();
    drop(cassie);
    // Act
    let restarted =
        Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default()).unwrap();
    restarted.startup().unwrap();
    let metadata = restarted.midge.load_constraints(CANONICAL).unwrap();
    let duplicate = restarted.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert_eq!(
        metadata,
        vec![expected],
        "persisted metadata must qualify canonical writes; duplicate outcome: {duplicate:?}"
    );
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "persisted owner must reject the duplicate: {duplicate:?}"
    );
}

#[test]
fn should_preserve_unique_backing_given_schema_qualified_metadata_when_local_storage_restarts() {
    // Arrange
    let directory = CatalogDirectory::new("constraint-namespace-schema_qualified");
    let (cassie, expected) = local_unpublished_fixture(&directory.0);
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("persisted".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    cassie
        .midge
        .save_constraints_with_unique_reservations(
            "public.namespace_probe",
            std::slice::from_ref(&expected),
        )
        .unwrap();
    drop(cassie);
    // Act
    let restarted =
        Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default()).unwrap();
    restarted.startup().unwrap();
    let metadata = restarted.midge.load_constraints(CANONICAL).unwrap();
    let duplicate = restarted.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert_eq!(
        metadata,
        vec![expected],
        "persisted metadata must qualify canonical writes; duplicate outcome: {duplicate:?}"
    );
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "persisted owner must reject the duplicate: {duplicate:?}"
    );
}

#[test]
fn should_preserve_unique_backing_given_database_qualified_metadata_when_local_storage_restarts() {
    // Arrange
    let directory = CatalogDirectory::new("constraint-namespace-database_qualified");
    let (cassie, expected) = local_unpublished_fixture(&directory.0);
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("persisted".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    cassie
        .midge
        .save_constraints_with_unique_reservations(
            "postgres.public.namespace_probe",
            std::slice::from_ref(&expected),
        )
        .unwrap();
    drop(cassie);
    // Act
    let restarted =
        Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default()).unwrap();
    restarted.startup().unwrap();
    let metadata = restarted.midge.load_constraints(CANONICAL).unwrap();
    let duplicate = restarted.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert_eq!(
        metadata,
        vec![expected],
        "persisted metadata must qualify canonical writes; duplicate outcome: {duplicate:?}"
    );
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "persisted owner must reject the duplicate: {duplicate:?}"
    );
}

#[test]
fn should_backfill_public_metadata_saves_given_unqualified_names_when_committed_owners_exist() {
    // Arrange
    let (cassie, constraint) = unpublished_fixture();
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("owner".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints("namespace_probe", std::slice::from_ref(&constraint))
        .unwrap();
    let duplicate = cassie.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "public metadata saves must publish committed owner backing: {duplicate:?}"
    );
}

#[test]
fn should_backfill_public_metadata_saves_given_schema_qualified_names_when_committed_owners_exist()
{
    // Arrange
    let (cassie, constraint) = unpublished_fixture();
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("owner".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints("public.namespace_probe", std::slice::from_ref(&constraint))
        .unwrap();
    let duplicate = cassie.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "public metadata saves must publish committed owner backing: {duplicate:?}"
    );
}

#[test]
fn should_backfill_public_metadata_saves_given_database_qualified_names_when_committed_owners_exist(
) {
    // Arrange
    let (cassie, constraint) = unpublished_fixture();
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("owner".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints(
            "postgres.public.namespace_probe",
            std::slice::from_ref(&constraint),
        )
        .unwrap();
    let duplicate = cassie.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "public metadata saves must publish committed owner backing: {duplicate:?}"
    );
}

#[test]
fn should_reject_unique_metadata_publication_given_duplicate_rows_when_public_save_is_used() {
    // Arrange
    let (cassie, constraint) = unpublished_fixture();
    for id in ["first", "second"] {
        cassie
            .midge
            .put_document(
                CANONICAL,
                Some(id.into()),
                serde_json::json!({"value":"same"}),
            )
            .unwrap();
    }
    // Act
    let result = cassie
        .midge
        .save_constraints("namespace_probe", std::slice::from_ref(&constraint));
    let published = cassie.midge.load_constraints(CANONICAL).unwrap();
    // Assert
    assert!(
        matches!(&result, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "invalid existing rows must reject publication: {result:?}"
    );
    assert!(
        published.is_empty(),
        "a failed validation must leave constraints unchanged"
    );
}

#[test]
fn should_read_one_constraint_set_given_equivalent_names_when_metadata_is_published() {
    // Arrange
    let (cassie, expected) = fixture("public.namespace_probe");
    // Act
    let sets = ["namespace_probe", "public.namespace_probe", CANONICAL]
        .map(|name| cassie.midge.load_constraints(name).unwrap());
    // Assert
    for actual in sets {
        assert_eq!(actual, vec![expected.clone()]);
    }
}

#[test]
fn should_release_alias_reservations_given_a_dropped_unique_when_the_key_is_republished() {
    // Arrange
    let (cassie, constraint) = fixture("namespace_probe");
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("old".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints_and_release_unique_reservations(
            "public.namespace_probe",
            &[],
            &["value".into()],
        )
        .unwrap();
    cassie.midge.delete_document(CANONICAL, "old").unwrap();
    cassie
        .midge
        .save_constraints("namespace_probe", std::slice::from_ref(&constraint))
        .unwrap();
    let inserted = cassie.midge.put_document(
        CANONICAL,
        Some("new".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        inserted.is_ok(),
        "removed owner reservation must not survive: {inserted:?}"
    );
    assert_eq!(
        cassie
            .midge
            .load_constraints("public.namespace_probe")
            .unwrap(),
        vec![constraint]
    );
}

#[test]
fn should_preserve_declared_primary_names_given_alias_publication_when_existing_owners_conflict() {
    // Arrange
    let (cassie, mut constraint) = unpublished_fixture();
    constraint.unique = false;
    constraint.unique_name = None;
    constraint.primary_key = true;
    constraint.primary_key_name = Some("namespace_primary".into());
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("owner".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints("namespace_probe", &[constraint])
        .unwrap();
    let duplicate = cassie.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_primary"),
        "named primary key must retain identity: {duplicate:?}"
    );
}

#[test]
fn should_isolate_constraint_owners_given_equal_relation_names_when_schemas_and_databases_differ() {
    // Arrange
    let (cassie, original) = fixture("namespace_probe");
    cassie.midge.create_database("other", None).unwrap();
    cassie.midge.create_namespace("other").unwrap();
    let cases = [
        ("namespace_probe", "namespace_unique"),
        ("other.namespace_probe", "schema_unique"),
        ("other.public.namespace_probe", "database_unique"),
    ];
    for (collection, name) in &cases[1..] {
        cassie
            .midge
            .create_collection(collection, crate::support_constraint_namespace::schema())
            .unwrap();
        let mut constraint = original.clone();
        constraint.unique_name = Some((*name).into());
        cassie
            .midge
            .save_constraints(collection, &[constraint])
            .unwrap();
    }
    // Act
    let observations: Vec<_> = cases
        .iter()
        .map(|(collection, _)| {
            let metadata = cassie.midge.load_constraints(collection).unwrap();
            let first = cassie.midge.put_document(
                collection,
                Some("owner".into()),
                serde_json::json!({"value":"same"}),
            );
            let duplicate = cassie.midge.put_document(
                collection,
                Some("duplicate".into()),
                serde_json::json!({"value":"same"}),
            );
            (metadata, first, duplicate)
        })
        .collect();
    // Assert
    for ((_, expected), (metadata, first, duplicate)) in cases.iter().zip(observations) {
        assert_eq!(metadata[0].unique_name.as_deref(), Some(*expected));
        assert!(
            first.is_ok(),
            "equal values in different namespaces must be independent: {first:?}"
        );
        assert!(
            matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == expected),
            "the namespace's named owner must reject its duplicate: {duplicate:?}"
        );
    }
}

#[test]
fn should_replay_canonical_cleanup_given_an_alias_drop_when_local_storage_restarts() {
    // Arrange
    let directory = CatalogDirectory::new("constraint-alias-cleanup");
    let (cassie, constraint) = local_unpublished_fixture(&directory.0);
    cassie
        .midge
        .save_constraints(CANONICAL, std::slice::from_ref(&constraint))
        .unwrap();
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("old".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    cassie::midge::adapter::set_unique_constraint_cleanup_failure_point(true);
    // Act
    let interrupted = cassie
        .midge
        .save_constraints_and_release_unique_reservations(
            "public.namespace_probe",
            &[],
            &["value".into()],
        );
    drop(cassie);
    let recovered =
        Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default()).unwrap();
    recovered.startup().unwrap();
    let metadata = recovered.midge.load_constraints(CANONICAL).unwrap();
    recovered.midge.delete_document(CANONICAL, "old").unwrap();
    recovered
        .midge
        .save_constraints("namespace_probe", &[constraint])
        .unwrap();
    let inserted = recovered.midge.put_document(
        CANONICAL,
        Some("new".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(interrupted, Err(CassieError::Execution(message)) if message.contains("injected test failure"))
    );
    assert!(
        metadata.is_empty(),
        "the dropped canonical constraint must stay dropped after recovery"
    );
    assert!(
        inserted.is_ok(),
        "replayed cleanup must retire the old canonical owner: {inserted:?}"
    );
}

#[test]
fn should_preserve_backing_given_repeated_publication_when_no_unique_field_is_added() {
    // Arrange
    let (cassie, mut constraint) = fixture("namespace_probe");
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("owner".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    constraint.default_value = Some(serde_json::json!("default"));
    // Act
    cassie
        .midge
        .save_constraints("public.namespace_probe", std::slice::from_ref(&constraint))
        .unwrap();
    cassie
        .midge
        .save_constraints_with_unique_reservations(CANONICAL, std::slice::from_ref(&constraint))
        .unwrap();
    let metadata = cassie.midge.load_constraints("namespace_probe").unwrap();
    let duplicate = cassie.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert_eq!(metadata, vec![constraint]);
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "repeated metadata updates must retain the committed owner: {duplicate:?}"
    );
}

#[test]
fn should_preserve_backing_given_empty_cleanup_when_no_unique_field_is_removed() {
    // Arrange
    let (cassie, constraint) = fixture("namespace_probe");
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("owner".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints_and_release_unique_reservations(
            "public.namespace_probe",
            std::slice::from_ref(&constraint),
            &[],
        )
        .unwrap();
    let metadata = cassie.midge.load_constraints("namespace_probe").unwrap();
    let duplicate = cassie.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert_eq!(metadata, vec![constraint]);
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "empty cleanup must retain the committed owner: {duplicate:?}"
    );
}

#[test]
fn should_keep_unique_enforcement_given_concurrent_publication_and_direct_write() {
    // Arrange
    let (cassie, constraint) = unpublished_fixture();
    let cassie = std::sync::Arc::new(cassie);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let publisher_cassie = std::sync::Arc::clone(&cassie);
    let publisher_barrier = std::sync::Arc::clone(&barrier);
    let publication_worker = std::thread::spawn(move || {
        publisher_barrier.wait();
        publisher_cassie
            .midge
            .save_constraints("namespace_probe", &[constraint])
    });
    let writer_cassie = std::sync::Arc::clone(&cassie);
    let writer_barrier = std::sync::Arc::clone(&barrier);
    let writer = std::thread::spawn(move || {
        writer_barrier.wait();
        writer_cassie.midge.put_document(
            CANONICAL,
            Some("owner".into()),
            serde_json::json!({"value":"same"}),
        )
    });

    // Act
    barrier.wait();
    let publication_result = publication_worker.join().unwrap();
    let inserted = writer.join().unwrap();
    let duplicate = cassie.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );

    // Assert
    assert!(
        publication_result.is_ok(),
        "constraint publication failed: {publication_result:?}"
    );
    assert!(
        inserted.is_ok()
            || matches!(&inserted, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "the concurrent write had an unexpected result: {inserted:?}"
    );
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "the completed publication must reject the next duplicate: {duplicate:?}"
    );
}

#[test]
fn should_backfill_new_unique_owners_given_empty_cleanup_when_existing_rows_are_published() {
    // Arrange
    let (cassie, constraint) = unpublished_fixture();
    cassie
        .midge
        .put_document(
            CANONICAL,
            Some("owner".into()),
            serde_json::json!({"value":"same"}),
        )
        .unwrap();
    // Act
    cassie
        .midge
        .save_constraints_and_release_unique_reservations(
            "public.namespace_probe",
            &[constraint],
            &[],
        )
        .unwrap();
    let duplicate = cassie.midge.put_document(
        CANONICAL,
        Some("duplicate".into()),
        serde_json::json!({"value":"same"}),
    );
    // Assert
    assert!(
        matches!(&duplicate, Err(CassieError::UniqueViolation { constraint, .. }) if constraint == "namespace_unique"),
        "empty cleanup must use the public validating/backfilling save: {duplicate:?}"
    );
}
