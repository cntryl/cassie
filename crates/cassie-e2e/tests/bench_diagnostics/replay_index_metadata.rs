//! Logical definition normalization must preserve every non-hydration field.
#[path = "../../benches/tier3_replay_durability/read_campaign/index_attestation.rs"]
mod index_attestation;

use cassie::catalog::{IndexKind, IndexMeta};
use index_attestation::verify_definitions;
use std::collections::BTreeMap;

fn definitions() -> (IndexMeta, IndexMeta) {
    let catalog = IndexMeta {
        definition_semantics_version: 1,
        collection: "postgres.public.replay_bench".into(),
        name: "postgres.public.replay_bench_title_idx".into(),
        field: "title".into(),
        fields: vec!["title".into()],
        expressions: vec![],
        include_fields: vec![],
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::from([
            ("__cassie_access_method".into(), "btree".into()),
            ("__cassie_catalog_oid".into(), "16399".into()),
        ]),
    };
    let mut persisted = catalog.clone();
    persisted
        .options
        .insert("__cassie_relation_id".into(), "47".into());
    persisted
        .options
        .insert("__cassie_storage_id".into(), "98".into());
    (catalog, persisted)
}

#[test]
fn should_match_hydrated_definitions_with_valid_native_identities() {
    // Arrange
    let (catalog, persisted) = definitions();
    let original = persisted.clone();

    // Act
    let observed =
        verify_definitions(&[catalog], std::slice::from_ref(&persisted), 47, 47).unwrap();

    // Assert
    assert_eq!(observed[0].relation_id, 47);
    assert_eq!(observed[0].storage_id, 98);
    assert_eq!(persisted, original);
    assert_eq!(persisted.options["__cassie_catalog_oid"], "16399");
    assert_eq!(
        verify_definitions(&[], &[], 47, 47).unwrap(),
        [] as [index_attestation::NativeIdentity; 0]
    );
}

#[test]
fn should_reject_semantic_definition_drift_outside_hydration_keys() {
    for case in [
        "field",
        "fields",
        "expressions",
        "include_fields",
        "predicate",
        "kind",
        "unique",
        "semantic_version",
        "access_method",
        "catalog_oid",
        "other_internal_option",
        "name",
        "collection",
    ] {
        // Arrange
        let (catalog, mut persisted) = definitions();
        match case {
            "field" => persisted.field = "wrong".into(),
            "fields" => persisted.fields.push("wrong".into()),
            "expressions" => persisted.expressions.push("wrong".into()),
            "include_fields" => persisted.include_fields.push("wrong".into()),
            "predicate" => persisted.predicate = Some("false".into()),
            "kind" => persisted.kind = IndexKind::FullText,
            "unique" => persisted.unique = true,
            "semantic_version" => persisted.definition_semantics_version = 2,
            "access_method" => {
                persisted
                    .options
                    .insert("__cassie_access_method".into(), "hash".into());
            }
            "catalog_oid" => {
                persisted
                    .options
                    .insert("__cassie_catalog_oid".into(), "16400".into());
            }
            "other_internal_option" => {
                persisted
                    .options
                    .insert("__cassie_semantic_guard".into(), "wrong".into());
            }
            "name" => persisted.name = "wrong".into(),
            "collection" => persisted.collection = "wrong".into(),
            _ => unreachable!(),
        }

        // Act
        let result = verify_definitions(&[catalog], &[persisted], 47, 47);

        // Assert
        assert!(result.unwrap_err().contains("definition"), "case {case}");
    }
}

#[test]
fn should_reject_invalid_persisted_index_identities() {
    for (key, value) in [
        ("__cassie_relation_id", None),
        ("__cassie_relation_id", Some("0")),
        ("__cassie_relation_id", Some("not-an-id")),
        ("__cassie_relation_id", Some("48")),
        ("__cassie_storage_id", None),
        ("__cassie_storage_id", Some("0")),
        ("__cassie_storage_id", Some("not-an-id")),
    ] {
        // Arrange
        let (catalog, mut persisted) = definitions();
        if let Some(value) = value {
            persisted.options.insert(key.into(), value.into());
        } else {
            persisted.options.remove(key);
        }

        // Act
        let result = verify_definitions(&[catalog], &[persisted], 47, 47);

        // Assert
        assert!(result.is_err(), "identity case {key}={value:?}");
    }
}

#[test]
fn should_require_matching_nonzero_collection_identities() {
    for (catalog_id, native_id) in [(0, 47), (47, 0), (47, 48)] {
        // Arrange
        let (catalog, persisted) = definitions();

        // Act
        let result = verify_definitions(&[catalog], &[persisted], catalog_id, native_id);

        // Assert
        assert!(result.unwrap_err().contains("collection"));
    }
}
