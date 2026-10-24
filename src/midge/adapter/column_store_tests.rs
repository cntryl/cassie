use super::{FieldSchema, Midge, Schema, StorageFamily};
use crate::catalog::{DefaultSequenceOwnership, FieldConstraint, IdentityGeneration, SequenceMeta};

#[test]
fn should_persist_relation_catalog_in_database_family() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie_relation_catalog_family_{}",
        uuid::Uuid::new_v4()
    ));
    let midge = Midge::new_with_data_dir(&path).expect("create Midge");
    let collection = midge.canonical_collection_name("family_catalog_items");
    let key = Midge::collection_schema_key(&collection);

    // Act
    midge
        .create_collection(
            "family_catalog_items",
            Schema {
                fields: vec![FieldSchema {
                    name: "id".to_string(),
                    data_type: crate::types::DataType::Int,
                    nullable: false,
                }],
            },
        )
        .expect("create collection");

    // Assert
    assert!(midge
        .raw_get(StorageFamily::Data, &key)
        .expect("read database-family catalog")
        .is_some());
    assert!(midge
        .raw_get(StorageFamily::Schema, &key)
        .expect("read schema-family catalog")
        .is_none());
}

#[test]
fn should_reject_identity_sequence_collision_before_collection_publication() {
    // Arrange
    let midge = Midge::new_in_memory("postgres").expect("create in-memory Midge");
    let table = "identity_sequence_collision";
    let sequence_name = "identity_sequence_collision_id_seq";
    let mut existing_sequence = SequenceMeta::new(sequence_name, crate::types::DataType::Int);
    existing_sequence.object_id = 42;
    midge
        .put_sequence(&existing_sequence)
        .expect("seed the colliding sequence metadata");

    let mut constraint = FieldConstraint::new("id");
    constraint.not_null = true;
    constraint.identity = Some(IdentityGeneration::ByDefault);
    constraint.default_sequence = Some(sequence_name.to_string());
    constraint.default_sequence_owned = DefaultSequenceOwnership::owned();
    constraint.default_expression =
        Some(crate::catalog::canonical_nextval_expression(sequence_name));
    let mut sequence = SequenceMeta::new(sequence_name, crate::types::DataType::Int);
    sequence.owned_by = Some(crate::catalog::SequenceOwner {
        table: table.to_string(),
        column: "id".to_string(),
    });
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "id".to_string(),
            data_type: crate::types::DataType::Int,
            nullable: false,
        }],
    };
    let metadata = crate::catalog::CollectionMeta::new(table, None);

    // Act
    let create_error = midge
        .create_collection_with_meta_and_constraints_and_sequences_if_absent(
            table,
            &schema,
            &metadata,
            &[constraint],
            &[sequence],
        )
        .expect_err("a sequence collision must abort collection publication");

    // Assert
    let collection_key = Midge::collection_schema_key(&midge.canonical_collection_name(table));
    let constraint_key = Midge::constraints_key(&midge.canonical_collection_name(table));
    assert!(matches!(
        create_error,
        crate::app::CassieError::CatalogObjectAlreadyExists {
            kind: crate::app::CatalogObjectKind::Sequence,
            ..
        }
    ));
    assert!(midge
        .raw_get(StorageFamily::Data, &collection_key)
        .expect("read collection schema")
        .is_none());
    assert!(midge
        .raw_get(StorageFamily::Data, &constraint_key)
        .expect("read collection constraints")
        .is_none());
    assert_eq!(
        midge
            .get_sequence(sequence_name)
            .expect("read existing sequence")
            .expect("existing sequence remains present")
            .object_id,
        42
    );
}
