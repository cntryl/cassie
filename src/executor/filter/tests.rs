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
fn should_share_analyzer_normalization_across_search_context_sources() {
    // Arrange
    for case_folding in [true, false] {
        let analyzer = AnalyzerConfig {
            case_folding,
            ..AnalyzerConfig::default()
        };
        let field_analyzer = HashMap::from([("body".to_string(), analyzer.clone())]);
        let row = BatchRow::new(vec![(
            "body".to_string(),
            Value::String("Rust compiler".to_string()),
        )]);
        let text_fields = vec!["body".to_string()];
        let row_context = SearchContext::from_rows(
            std::iter::once(&row),
            &text_fields,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &field_analyzer,
        );
        let source_stats =
            SearchTermStats::from_text_with_analyzer(Some("Rust compiler"), &analyzer);
        let persisted_frequency = source_stats
            .term_counts()
            .keys()
            .map(|term| (term.clone(), 1))
            .collect::<std::collections::BTreeMap<_, _>>();
        let numeric_options = HashMap::new();
        let persisted_context = SearchContext::from_persisted_field_statistics(
            "body",
            &PersistedFieldStatistics {
                total_documents: 1,
                average_document_length: 2.0,
                document_frequency: &persisted_frequency,
                field_boost: &numeric_options,
                field_k1: &numeric_options,
                field_b: &numeric_options,
                field_analyzer: &field_analyzer,
            },
        );
        let query = if case_folding { "rust" } else { "Rust" };
        let query_terms = prepare_query_terms_with_analyzer(query, &analyzer);

        // Act
        let row_score = row_context.score_term_stats(Some("body"), &source_stats, &query_terms);
        let persisted_score =
            persisted_context.score_term_stats(Some("body"), &source_stats, &query_terms);

        // Assert
        assert!(row_score > 0.0);
        assert!((row_score - persisted_score).abs() < f64::EPSILON);
    }
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

#[test]
fn should_return_unknown_for_incompatible_scalar_comparisons() {
    // Arrange
    let pairs = [
        (ScalarValue::Int(1), ScalarValue::Str("1".to_string())),
        (ScalarValue::Float(1.0), ScalarValue::Str("1".to_string())),
        (
            ScalarValue::Bool(true),
            ScalarValue::Str("true".to_string()),
        ),
    ];

    // Act
    let results = pairs.map(|(left, right)| {
        (
            eq_value(&left, &right),
            ordered_cmp(&left, &right, std::cmp::Ordering::is_lt),
        )
    });

    // Assert
    assert_eq!(results, [(None, None), (None, None), (None, None)]);
}

#[test]
fn should_preserve_boolean_numeric_equality_coercions() {
    // Arrange
    let comparisons = [
        (ScalarValue::Bool(true), ScalarValue::Int(1)),
        (ScalarValue::Bool(false), ScalarValue::Int(0)),
        (ScalarValue::Int(2), ScalarValue::Bool(true)),
        (ScalarValue::Int(0), ScalarValue::Bool(true)),
        (ScalarValue::Bool(true), ScalarValue::Float(0.5)),
        (ScalarValue::Float(0.0), ScalarValue::Bool(false)),
    ];

    // Act
    let results = comparisons.map(|(left, right)| eq_value(&left, &right));

    // Assert
    assert_eq!(
        results,
        [
            Some(true),
            Some(true),
            Some(true),
            Some(false),
            Some(true),
            Some(true),
        ]
    );
}
