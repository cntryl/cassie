use crate::inverted_index::InvertedIndex;
use crate::{bm25_score, tokenize};

#[test]
fn should_tokenize_each_declared_hotpath_input() {
    // Arrange
    let inputs = [
        "Alpha beta, gamma and delta",
        "Cassie plans selective indexed queries",
        "Midge persists ordered document keys",
        "Hybrid retrieval combines text and vectors",
        "PostgreSQL clients reuse prepared statements",
        "Column batches accelerate analytical filters",
        "Snapshots preserve catalog and row state",
        "Operators inspect bounded runtime metrics",
    ];

    // Act
    let token_count = inputs
        .iter()
        .map(|input| tokenize(input).len())
        .sum::<usize>();

    // Assert
    assert!(token_count > 0);
}

#[test]
fn should_score_each_declared_bm25_term() {
    // Arrange
    let inputs = [
        (3.0, 10.0, 1_000.0, 120.0, 100.0),
        (4.0, 11.0, 1_000.0, 118.0, 100.0),
        (5.0, 12.0, 1_000.0, 116.0, 100.0),
        (6.0, 13.0, 1_000.0, 114.0, 100.0),
        (7.0, 14.0, 1_000.0, 112.0, 100.0),
        (8.0, 15.0, 1_000.0, 110.0, 100.0),
        (9.0, 16.0, 1_000.0, 108.0, 100.0),
        (10.0, 17.0, 1_000.0, 106.0, 100.0),
    ];

    // Act
    let scores = inputs.map(|(tf, df, n, dl, avgdl)| bm25_score(tf, df, n, 1.2, 0.75, dl, avgdl));

    // Assert
    assert_eq!(scores.len(), 8);
    assert!(scores.iter().all(|score| score.is_finite() && *score > 0.0));
}

#[test]
fn should_merge_unique_posting_documents_for_both_query_terms() {
    // Arrange
    let mut index = InvertedIndex::default();
    for row in 0..4 {
        let terms = if row % 2 == 0 {
            vec!["alpha".to_owned(), "common".to_owned()]
        } else {
            vec!["beta".to_owned(), "common".to_owned()]
        };
        index.index_document(&format!("doc-{row}"), &terms);
    }

    // Act
    let candidates = index.candidate_documents(&["alpha".to_owned(), "beta".to_owned()]);

    // Assert
    assert_eq!(candidates.len(), 4);
}
