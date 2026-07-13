#[cfg(test)]
use super::*;
use serde_json::json;

#[test]
fn should_build_deterministic_lexkey_storage_keys() {
    // Arrange
    let left = row_key("events", "id-1");

    // Act
    let right = row_key("events", "id-1");
    let other_family = row_hash_key("events", "id-1");

    // Assert
    assert_eq!(left, right);
    assert_ne!(left, other_family);
    assert!(!left.starts_with(b"__cassie__/"));
    assert!(!left.starts_with(b"r/"));
}

#[test]
fn should_build_prefix_that_matches_only_child_keys() {
    // Arrange
    let prefix = row_prefix("orders");
    let matching = row_key("orders", "1");
    let sibling = row_key("orders-archive", "1");

    // Act
    let decoded = utf8_suffix_after_prefix(&matching, &prefix);

    // Assert
    assert!(matching.starts_with(&prefix));
    assert!(!sibling.starts_with(&prefix));
    assert_eq!(decoded.as_deref(), Some("1"));
}

#[test]
fn should_preserve_scalar_value_ordering() {
    // Arrange
    let values = vec![
        json!(null),
        json!(false),
        json!(true),
        json!(-10),
        json!(0),
        json!(7),
        json!(-1.25),
        json!(2.5),
        json!("a\u{0}a"),
        json!("aa"),
    ];

    // Act
    let encoded = values
        .iter()
        .map(|value| {
            let mut key = Vec::new();
            append_scalar_value(&mut key, value).expect("scalar value");
            key
        })
        .collect::<Vec<_>>();
    let mut sorted = encoded.clone();
    sorted.sort();

    // Assert
    assert_eq!(encoded, sorted);
}

#[test]
fn should_reject_unsupported_scalar_value_without_panicking() {
    // Arrange
    let value = json!([]);
    let mut key = Vec::new();

    // Act
    let result = append_scalar_value(&mut key, &value);

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_include_embedded_nul_text_in_scalar_order() {
    // Arrange
    let before = scalar_index_entry_key("events", "idx", &[json!("a\u{0}a")], "1").unwrap();
    let after = scalar_index_entry_key("events", "idx", &[json!("aa")], "1").unwrap();

    // Act
    let is_ordered = before < after;

    // Assert
    assert!(is_ordered);
}

#[test]
fn should_use_compact_internal_markers_for_frequently_used_path_components() {
    // Arrange
    let collection = "events";
    let index = "email_idx";
    let key = scalar_index_data_prefix(collection, index);
    let ts = time_series_index_data_prefix(collection, index);
    let row = column_store_row_prefix(collection);
    let metadata = column_batch_metadata_key(collection, index);
    let segment = column_batch_segment_key(collection, index, 1);
    let reservation_constraint =
        unique_constraint_reservation_key(collection, "email", &json!("tenant")).unwrap();
    let reservation_index =
        unique_scalar_index_reservation_key(collection, index, &[json!(1)]).unwrap();

    // Act
    let scalar_parts = key
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let ts_parts = ts
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let row_parts = row
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let metadata_parts = metadata
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let segment_parts = segment
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let reservation_constraint_parts = reservation_constraint
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let reservation_index_parts = reservation_index
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let graph_out = graph_outbound_prefix("social", "person", "a1");
    let graph_in = graph_inbound_prefix("social", "person", "a1");
    let graph_out_parts = graph_out
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let graph_in_parts = graph_in
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let fulltext_metadata = fulltext_index_key("events", "title_search");
    let fulltext_metadata_parts = fulltext_metadata
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let fulltext_manifest = fulltext_index_manifest_key("events", "title_search", 42);
    let fulltext_manifest_parts = fulltext_manifest
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let fulltext_terms = fulltext_term_postings_key("events", "title_search", "alpha");
    let fulltext_terms_parts = fulltext_terms
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();
    let fulltext_doc = fulltext_document_stats_key("events", "title_search", "doc-id-1");
    let fulltext_doc_parts = fulltext_doc
        .split(|byte| *byte == LexKey::SEPARATOR)
        .collect::<Vec<_>>();

    // Assert
    assert!(!scalar_parts.iter().any(|part| part == b"data"));
    assert!(!ts_parts.iter().any(|part| part == b"data"));
    assert!(!row_parts.iter().any(|part| part == b"row"));
    assert!(!row_parts.iter().any(|part| part == b"field"));
    assert!(!metadata_parts.iter().any(|part| part == b"metadata"));
    assert!(!segment_parts.iter().any(|part| part == b"segment"));
    assert!(!reservation_constraint_parts
        .iter()
        .any(|part| part == b"constraint"));
    assert!(!reservation_index_parts.iter().any(|part| part == b"index"));
    assert!(
        !graph_out_parts.iter().any(|part| part == b"out")
            && !graph_in_parts.iter().any(|part| part == b"in")
    );
    assert!(!fulltext_metadata_parts
        .iter()
        .any(|part| part == b"metadata"));
    assert!(fulltext_metadata_parts.contains(&FULLTEXT_ARTIFACT_META.as_slice()));
    assert!(!fulltext_manifest_parts
        .iter()
        .any(|part| part == b"manifest"));
    assert!(fulltext_manifest_parts.contains(&FULLTEXT_ARTIFACT_MANIFEST.as_slice()));
    assert!(fulltext_terms_parts.contains(&FULLTEXT_ARTIFACT_POSTINGS.as_slice()));
    assert!(!fulltext_terms_parts.iter().any(|part| part == b"postings"));
    assert!(fulltext_doc_parts.contains(&FULLTEXT_ARTIFACT_DOCUMENT.as_slice()));
    assert!(!fulltext_doc_parts.iter().any(|part| part == b"documents"));
    assert_eq!(FULLTEXT_ARTIFACT_META.len(), 1);
    assert_eq!(FULLTEXT_ARTIFACT_MANIFEST.len(), 1);
    assert_eq!(FULLTEXT_ARTIFACT_POSTINGS.len(), 1);
    assert_eq!(FULLTEXT_ARTIFACT_DOCUMENT.len(), 1);
}
