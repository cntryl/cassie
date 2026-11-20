use super::support_executor as support;
use super::support_fulltext_maintenance::search_metric;
use support::{cassie_temp, create_text_collection, put_document, put_fulltext_index};

use cassie::app::Cassie;

const COLLECTION: &str = "fulltext_write_amplification";
const INDEX: &str = "amplification_body_idx";
const SMALL_CORPUS: usize = 30;
const LARGE_CORPUS: usize = SMALL_CORPUS * 10;
const BLOCK_CAP_BYTES: u64 = 64 * 1024;
const BASELINE_BODY: &str = "needle thread0";

struct IndexWrite {
    documents: u64,
    pieces: u64,
    bytes: u64,
    terms: u64,
}

fn fixture(unrelated_documents: usize) -> Cassie {
    let cassie = cassie_temp("fulltext_write_amplification");
    create_text_collection(&cassie, COLLECTION, &["id", "body"]);
    put_fulltext_index(&cassie, COLLECTION, INDEX, "body", &[]);
    put_document(
        &cassie,
        COLLECTION,
        "target",
        serde_json::json!({"body": BASELINE_BODY}),
    );
    grow_unrelated(&cassie, 0, unrelated_documents);
    cassie
}

/// Adds unrelated documents that each carry unrelated vocabulary and a shared common term.
fn grow_unrelated(cassie: &Cassie, from: usize, to: usize) {
    for index in from..to {
        put_document(
            cassie,
            COLLECTION,
            &format!("unrelated-{index:05}"),
            serde_json::json!({"body": format!("bulk unrelatedterm{index} extra{index}")}),
        );
    }
}

fn last_write(cassie: &Cassie) -> IndexWrite {
    IndexWrite {
        documents: search_metric(cassie, "fulltext_maintenance_last_documents"),
        pieces: search_metric(cassie, "fulltext_maintenance_last_index_pieces_written"),
        bytes: search_metric(cassie, "fulltext_maintenance_last_index_bytes_written"),
        terms: search_metric(cassie, "fulltext_maintenance_terms_touched_total"),
    }
}

fn update_target(cassie: &Cassie, body: &str) -> IndexWrite {
    let terms_before = search_metric(cassie, "fulltext_maintenance_terms_touched_total");
    put_document(
        cassie,
        COLLECTION,
        "target",
        serde_json::json!({ "body": body }),
    );
    let mut write = last_write(cassie);
    write.terms -= terms_before;
    write
}

#[test]
fn should_not_grow_index_writes_with_unrelated_documents_and_vocabulary() {
    // Arrange
    let cassie = fixture(SMALL_CORPUS);
    let small = update_target(&cassie, "needle needle thread1");
    grow_unrelated(&cassie, SMALL_CORPUS, LARGE_CORPUS);
    update_target(&cassie, BASELINE_BODY);
    let rebuilds = search_metric(&cassie, "fulltext_rebuilds_total");

    // Act
    let large = update_target(&cassie, "needle needle thread1");

    // Assert
    assert_eq!(small.documents, 1);
    assert_eq!(large.documents, 1);
    assert_eq!(large.pieces, small.pieces, "pieces rewritten per update");
    assert!(
        large.bytes <= small.bytes + 16,
        "bytes written grew with the unrelated corpus: {} -> {}",
        small.bytes,
        large.bytes
    );
    assert_eq!(large.terms, small.terms, "terms rewritten per update");
    assert_eq!(search_metric(&cassie, "fulltext_rebuilds_total"), rebuilds);
}

#[test]
fn should_bound_a_common_term_update_by_one_posting_block() {
    // Arrange
    let cassie = fixture(SMALL_CORPUS);
    let small = update_target(&cassie, "needle bulk");
    grow_unrelated(&cassie, SMALL_CORPUS, LARGE_CORPUS);
    update_target(&cassie, BASELINE_BODY);

    // Act
    let large = update_target(&cassie, "needle bulk");

    // Assert
    assert_eq!(large.pieces, small.pieces, "pieces rewritten per update");
    assert!(
        large.bytes <= BLOCK_CAP_BYTES * 2,
        "a common-term update rewrites one block, not the corpus: {} bytes",
        large.bytes
    );
}

#[test]
fn should_count_index_writes_separately_from_source_rows() {
    // Arrange
    let cassie = fixture(SMALL_CORPUS);
    let operations = search_metric(&cassie, "fulltext_maintenance_operations_total");
    let documents = search_metric(&cassie, "fulltext_maintenance_documents_total");
    let pieces = search_metric(&cassie, "fulltext_maintenance_index_pieces_written_total");

    // Act
    put_document(
        &cassie,
        COLLECTION,
        "another",
        serde_json::json!({"body": "needle another"}),
    );

    // Assert
    let last = last_write(&cassie);
    assert_eq!(
        search_metric(&cassie, "fulltext_maintenance_operations_total"),
        operations + 1
    );
    assert_eq!(
        search_metric(&cassie, "fulltext_maintenance_documents_total"),
        documents + 1
    );
    assert_eq!(
        search_metric(&cassie, "fulltext_maintenance_index_pieces_written_total"),
        pieces + last.pieces
    );
    assert!(
        last.pieces >= 4,
        "statistics, two blocks, metadata at least"
    );
    assert!(last.bytes > 0);
}
