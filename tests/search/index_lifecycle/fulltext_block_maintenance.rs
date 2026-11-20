use super::support_executor as support;
use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;
use support::{cassie_temp, create_text_collection, put_fulltext_index};

const COLLECTION: &str = "fulltext_block_maintenance";
const INDEX: &str = "block_body_idx";

fn long_document_id(index: usize) -> String {
    format!("{index:03}-{}", format!("{index:03}").repeat(300))
}

fn fixture() -> Cassie {
    let cassie = cassie_temp("fulltext_block_maintenance");
    create_text_collection(&cassie, COLLECTION, &["id", "body"]);
    put_fulltext_index(&cassie, COLLECTION, INDEX, "body", &[]);
    cassie
}

fn put(cassie: &Cassie, index: usize, body: &str) {
    cassie
        .midge
        .put_document(
            COLLECTION,
            Some(long_document_id(index)),
            serde_json::json!({ "body": body }),
        )
        .expect("put document");
}

fn block_count(cassie: &Cassie) -> usize {
    let prefix = cassie
        .midge
        .fulltext_artifact_prefix_for_diagnostics(COLLECTION, INDEX)
        .expect("fulltext artifact prefix");
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("fulltext artifacts")
        .into_iter()
        .filter(|(_, value)| value.starts_with(b"FTB1"))
        .count()
}

fn assert_matches_clean_rebuild(cassie: &Cassie) {
    let maintained = cassie
        .midge
        .get_persisted_fulltext_index_state(COLLECTION, INDEX)
        .expect("maintained state")
        .expect("state exists");
    cassie
        .midge
        .rebuild_fulltext_index_for_diagnostics(COLLECTION, INDEX)
        .expect("rebuild");
    let rebuilt = cassie
        .midge
        .get_persisted_fulltext_index_state(COLLECTION, INDEX)
        .expect("rebuilt state")
        .expect("state exists");
    assert_eq!(maintained, rebuilt);
}

#[test]
fn should_split_a_growing_term_into_bounded_blocks_matching_a_rebuild() {
    // Arrange
    let cassie = fixture();

    // Act
    for index in 0..120 {
        put(&cassie, index, "alpha");
    }
    let blocks = block_count(&cassie);

    // Assert
    assert!(blocks > 1, "a block of ~150 KiB of ids must have split");
    let state = cassie
        .midge
        .get_persisted_fulltext_index_state(COLLECTION, INDEX)
        .expect("state is consistent")
        .expect("state exists");
    assert_eq!(state.postings["alpha"].len(), 120);
    assert_matches_clean_rebuild(&cassie);
}

#[test]
fn should_insert_between_or_before_existing_blocks_matching_a_rebuild() {
    // Arrange
    let cassie = fixture();
    for index in (0..120).step_by(2).map(|index| index + 10) {
        put(&cassie, index, "alpha");
    }

    // Act
    for index in [5, 11, 61, 99, 131, 1, 0] {
        put(&cassie, index, "alpha");
    }

    // Assert
    assert_matches_clean_rebuild(&cassie);
}

#[test]
fn should_merge_shrunken_blocks_matching_a_rebuild() {
    // Arrange
    let cassie = fixture();
    for index in 0..120 {
        put(&cassie, index, "alpha");
    }
    let grown = block_count(&cassie);

    // Act
    for index in (0..120).rev() {
        if index % 40 != 0 {
            cassie
                .midge
                .delete_document(COLLECTION, &long_document_id(index))
                .expect("delete document");
        }
    }

    // Assert
    assert!(block_count(&cassie) < grown, "emptied blocks are removed");
    assert_matches_clean_rebuild(&cassie);
}

#[test]
fn should_remove_every_artifact_of_a_term_when_its_last_document_leaves() {
    // Arrange
    let cassie = fixture();
    put(&cassie, 1, "alpha unique");
    put(&cassie, 2, "alpha");

    // Act
    put(&cassie, 1, "beta");

    // Assert
    let state = cassie
        .midge
        .get_persisted_fulltext_index_state(COLLECTION, INDEX)
        .expect("state is consistent")
        .expect("state exists");
    assert!(!state.postings.contains_key("unique"));
    assert_eq!(
        cassie
            .midge
            .fulltext_index_terms(COLLECTION, INDEX, "", 100)
            .expect("enumerate terms"),
        vec![("alpha".to_string(), 1), ("beta".to_string(), 1),]
    );
    assert_matches_clean_rebuild(&cassie);
}

#[test]
fn should_enumerate_index_terms_by_prefix_with_a_limit() {
    // Arrange
    let cassie = fixture();
    put(&cassie, 1, "stone stonework store storm apple");
    put(&cassie, 2, "stone");

    // Act
    let prefixed = cassie
        .midge
        .fulltext_index_terms(COLLECTION, INDEX, "ston", 10)
        .expect("enumerate prefix");
    let limited = cassie
        .midge
        .fulltext_index_terms(COLLECTION, INDEX, "st", 2)
        .expect("enumerate limited");

    // Assert
    assert_eq!(
        prefixed,
        vec![("stone".to_string(), 2), ("stonework".to_string(), 1)]
    );
    assert_eq!(limited.len(), 2);
}
