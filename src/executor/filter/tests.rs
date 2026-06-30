use super::*;
use crate::executor::batch::BatchRow;

#[test]
fn should_score_term_stats_same_as_text_scoring() {
    // Arrange
    let row = BatchRow::new(vec![(
        "body".to_string(),
        Value::String("alpha beta alpha".to_string()),
    )]);
    let text_fields = vec!["body".to_string()];
    let search_context = SearchContext::from_rows(
        std::iter::once(&row),
        &text_fields,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    );
    let term_stats = SearchTermStats::from_text(Some("alpha beta alpha"));
    let query_terms = prepare_query_terms("alpha beta");

    // Act
    let direct_score = search_context.score_text(Some("body"), "alpha beta alpha", "alpha beta");
    let stats_score = search_context.score_term_stats(Some("body"), &term_stats, &query_terms);

    // Assert
    assert!((direct_score - stats_score).abs() < f64::EPSILON);
}

#[test]
fn should_build_search_context_from_term_stats_with_same_statistics_as_rows() {
    // Arrange
    let rows = [
        BatchRow::new(vec![(
            "body".to_string(),
            Value::String("alpha beta".to_string()),
        )]),
        BatchRow::new(vec![("body".to_string(), Value::String(String::new()))]),
        BatchRow::new(vec![("body".to_string(), Value::Null)]),
    ];
    let text_fields = vec!["body".to_string()];
    let row_context = SearchContext::from_rows(
        rows.iter(),
        &text_fields,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    );
    let term_stats = [
        SearchTermStats::from_text(Some("alpha beta")),
        SearchTermStats::from_text(Some("")),
        SearchTermStats::from_text(None),
    ];

    // Act
    let stats_context = SearchContext::from_term_stats(
        "body",
        term_stats.iter(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    );

    // Assert
    assert_eq!(row_context.total_documents, stats_context.total_documents);
    assert_eq!(row_context.doc_frequency, stats_context.doc_frequency);
    assert_eq!(row_context.avg_doc_length, stats_context.avg_doc_length);
}

#[test]
fn should_score_single_field_term_stats_same_as_generic_context_with_custom_options() {
    // Arrange
    let documents = [
        SearchTermStats::from_text(Some("alpha beta alpha")),
        SearchTermStats::from_text(Some("alpha gamma")),
        SearchTermStats::from_text(Some("beta gamma")),
    ];
    let query_terms = prepare_query_terms("alpha beta");
    let source_stats = SearchTermStats::from_text(Some("alpha beta alpha"));
    let mut field_boost = HashMap::new();
    field_boost.insert("body".to_string(), 2.5);
    let mut field_k1 = HashMap::new();
    field_k1.insert("body".to_string(), 1.7);
    let mut field_b = HashMap::new();
    field_b.insert("body".to_string(), 0.3);
    let generic_context = SearchContext::from_term_stats(
        "body",
        documents.iter(),
        &field_boost,
        &field_k1,
        &field_b,
        &HashMap::new(),
    );
    let single_field_context = SingleFieldSearchContext::from_term_stats(
        "body",
        documents.iter(),
        &field_boost,
        &field_k1,
        &field_b,
    );

    // Act
    let generic_score = generic_context.score_term_stats(Some("body"), &source_stats, &query_terms);
    let single_field_score = single_field_context.score_term_stats(&source_stats, &query_terms);

    // Assert
    assert!((generic_score - single_field_score).abs() < f64::EPSILON);
}

#[test]
fn should_score_single_field_term_stats_as_zero_for_empty_or_missing_text() {
    // Arrange
    let documents = [
        SearchTermStats::from_text(Some("alpha beta")),
        SearchTermStats::from_text(Some("gamma")),
    ];
    let query_terms = prepare_query_terms("alpha");
    let context = SingleFieldSearchContext::from_term_stats(
        "body",
        documents.iter(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
    );
    let empty_stats = SearchTermStats::from_text(Some(""));
    let missing_stats = SearchTermStats::from_text(None);

    // Act
    let empty_score = context.score_term_stats(&empty_stats, &query_terms);
    let missing_score = context.score_term_stats(&missing_stats, &query_terms);

    // Assert
    assert!(empty_score.abs() < f64::EPSILON);
    assert!(missing_score.abs() < f64::EPSILON);
}
