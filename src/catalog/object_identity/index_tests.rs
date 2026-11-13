use super::{catalog_index_oid, ObjectScope};
use crate::catalog::{
    canonical_relation_name, Catalog, ExtensionMeta, IndexKind, IndexMeta,
    VectorAnnCatalogIdentityIds, VectorCatalogIdentitySet, VectorDistanceOperatorIds,
};
use crate::sql::binder::BindingContext;
use crate::types::ObjectIdKind;

fn index(kind: IndexKind, name: &str) -> IndexMeta {
    IndexMeta {
        definition_semantics_version: 0,
        collection: canonical_relation_name("object_identity_test", "public", "events"),
        name: name.to_string(),
        field: "event_at".to_string(),
        fields: vec!["event_at".to_string()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind,
        unique: false,
        options: std::collections::BTreeMap::new(),
    }
}

fn vector_extension() -> ExtensionMeta {
    let ann_identities = |access_method| VectorAnnCatalogIdentityIds {
        access_method,
        l2_operator_family: access_method + 1,
        inner_product_operator_family: access_method + 2,
        cosine_operator_family: access_method + 3,
        l2_operator_class: access_method + 4,
        inner_product_operator_class: access_method + 5,
        cosine_operator_class: access_method + 6,
    };
    ExtensionMeta {
        format_version: 2,
        database: "object_identity_test".to_string(),
        name: "vector".to_string(),
        version: "0.8.7".to_string(),
        schema: "object_identity_test.public".to_string(),
        owner_oid: 10,
        object_id: 1,
        digest_text_object_id: None,
        digest_bytea_object_id: None,
        vector_type_object_id: Some(2),
        vector_catalog_identities: Some(VectorCatalogIdentitySet {
            operators: VectorDistanceOperatorIds {
                l2: 3,
                inner_product: 4,
                cosine: 5,
            },
            hnsw: ann_identities(10),
            ivfflat: ann_identities(20),
        }),
    }
}

#[test]
fn should_resolve_only_index_relations_published_in_postgresql_catalogs() {
    // Arrange
    let catalog = Catalog::new();
    let column_index = index(IndexKind::Column, "events_column_idx");
    let time_series_index = index(IndexKind::TimeSeries, "events_time_series_idx");
    let scalar_index = index(IndexKind::Scalar, "events_btree_idx");
    let legacy_vector_index = index(IndexKind::Vector, "events_legacy_vector_idx");
    let mut hnsw_index = index(IndexKind::Vector, "events_hnsw_idx");
    hnsw_index.options.insert(
        IndexMeta::ACCESS_METHOD_OPTION.to_string(),
        "hnsw".to_string(),
    );
    catalog.register_index(column_index.clone());
    catalog.register_index(time_series_index.clone());
    catalog.register_index(scalar_index.clone());
    catalog.register_index(legacy_vector_index.clone());
    catalog.register_index(hnsw_index.clone());
    let context = BindingContext::new("object_identity_test", vec!["public".to_string()]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let column_name = scope
        .resolve(ObjectIdKind::RegClass, "events_column_idx")
        .expect("parse Column index identity");
    let time_series_name = scope
        .resolve(ObjectIdKind::RegClass, "events_time_series_idx")
        .expect("parse TimeSeries index identity");
    let scalar_name = scope
        .resolve(ObjectIdKind::RegClass, "events_btree_idx")
        .expect("parse scalar index identity");
    let legacy_vector_name = scope
        .resolve(ObjectIdKind::RegClass, "events_legacy_vector_idx")
        .expect("parse legacy Vector index identity");
    let hnsw_name_before_install = scope
        .resolve(ObjectIdKind::RegClass, "events_hnsw_idx")
        .expect("parse HNSW index identity before extension install");
    let column_label = scope.label(ObjectIdKind::RegClass, catalog_index_oid(&column_index));
    let time_series_label = scope.label(
        ObjectIdKind::RegClass,
        catalog_index_oid(&time_series_index),
    );
    let legacy_vector_label = scope.label(
        ObjectIdKind::RegClass,
        catalog_index_oid(&legacy_vector_index),
    );

    // Install extension identities before resolving the selected pgvector method.
    catalog.register_extension(vector_extension());
    let hnsw_name_after_install = scope
        .resolve(ObjectIdKind::RegClass, "events_hnsw_idx")
        .expect("resolve HNSW only after vector identities are installed");
    let hnsw_label = scope.label(ObjectIdKind::RegClass, catalog_index_oid(&hnsw_index));

    // Assert
    assert_eq!(column_name, None);
    assert_eq!(time_series_name, None);
    assert!(scalar_name.is_some());
    assert_eq!(legacy_vector_name, None);
    assert_eq!(hnsw_name_before_install, None);
    assert_eq!(
        hnsw_name_after_install,
        Some(catalog_index_oid(&hnsw_index))
    );
    assert_eq!(column_label, catalog_index_oid(&column_index).to_string());
    assert_eq!(
        time_series_label,
        catalog_index_oid(&time_series_index).to_string()
    );
    assert_eq!(
        legacy_vector_label,
        catalog_index_oid(&legacy_vector_index).to_string()
    );
    assert_eq!(hnsw_label, "events_hnsw_idx");
}

#[test]
fn should_derive_distinct_name_oids_outside_the_allocator_range() {
    // Arrange
    // These two names shared one OID under the former 800,000-slot hash.
    let names = ["cx_3327", "cx_5040"];

    // Act
    let oids = names.map(super::stable_index_oid);

    // Assert
    assert_ne!(oids[0], oids[1]);
    assert!(oids.iter().all(|oid| *oid >= 0x8000_0000));
}
