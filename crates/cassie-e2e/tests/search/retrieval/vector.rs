use cassie::hybrid::hybrid_score;
use cassie::search::analyzer::AnalyzerConfig;
use cassie::search::bm25;
use cassie::search::tokenizer;
use cassie::vector::{cosine_distance, dot_distance, dot_score, l2_distance};

fn assert_f64_close(actual: f64, expected: f64) {
    let tolerance = f64::EPSILON.max(expected.abs() * 1e-12);
    assert!(
        (actual - expected).abs() <= tolerance,
        "expected {actual} to equal {expected}"
    );
}

fn usize_to_f64(value: usize) -> f64 {
    value
        .to_string()
        .parse::<f64>()
        .expect("test count should fit f64")
}

#[test]
fn should_tokenize_text_into_lowercase_terms() {
    // Arrange
    let input = "Hello, THE world and the universe";

    // Act
    let tokens = tokenizer::tokenize(input);

    // Assert
    assert_eq!(tokens, vec!["hello", "world", "universe"]);
}

#[test]
fn should_compute_vector_distances_deterministically() {
    // Arrange
    let a = vec![1.0f32, 2.0, 3.0];
    let b = vec![1.0f32, 2.0, 3.0];
    let c = vec![4.0f32, 5.0, 6.0];

    // Act
    let same_distance = l2_distance(&a, &b);
    let different_distance = l2_distance(&a, &c);
    let cosine = cosine_distance(&a, &a);
    let dot_distance_score = dot_distance(&a, &a);
    let dot = dot_score(&a, &b);

    // Assert
    assert_f64_close(same_distance, 0.0);
    assert_f64_close(different_distance, 5.196_152_422_706_632);
    assert_f64_close(cosine, 0.0);
    assert_f64_close(dot_distance_score, -14.0);
    assert_f64_close(dot, 14.0);
}

#[test]
fn should_compute_vector_distances_with_simd_tail_elements() {
    // Arrange
    let a = vec![1.0f32; 9];
    let b = vec![2.0f32; 9];

    // Act
    let l2 = l2_distance(&a, &b);
    let cosine = cosine_distance(&a, &b);
    let dot = dot_score(&a, &b);

    // Assert
    assert_f64_close(l2, 3.0);
    assert_f64_close(cosine, 0.0);
    assert_f64_close(dot, 18.0);
}

#[test]
fn should_keep_high_magnitude_vector_distances_finite_across_simd_dimensions() {
    // Arrange
    let component = 2.0e19_f32;
    let scalar = vec![component; 4];
    let simd = vec![component; 8];
    let simd_with_tail = vec![component; 9];
    let zero = vec![0.0_f32; 8];
    let zero_with_tail = vec![0.0_f32; 9];
    let component_f64 = f64::from(component);

    // Act
    let scalar_dot = dot_score(&scalar, &scalar);
    let simd_dot = dot_score(&simd, &simd);
    let simd_tail_dot = dot_score(&simd_with_tail, &simd_with_tail);
    let scalar_l2 = l2_distance(&scalar, &[0.0_f32; 4]);
    let simd_l2 = l2_distance(&simd, &zero);
    let simd_tail_l2 = l2_distance(&simd_with_tail, &zero_with_tail);
    let scalar_cosine = cosine_distance(&scalar, &scalar);
    let simd_cosine = cosine_distance(&simd, &simd);
    let simd_tail_cosine = cosine_distance(&simd_with_tail, &simd_with_tail);

    // Assert
    assert_f64_close(scalar_dot, 4.0 * component_f64 * component_f64);
    assert_f64_close(simd_dot, 8.0 * component_f64 * component_f64);
    assert_f64_close(simd_tail_dot, 9.0 * component_f64 * component_f64);
    assert_f64_close(scalar_l2, 2.0 * component_f64);
    assert_f64_close(simd_l2, 8.0_f64.sqrt() * component_f64);
    assert_f64_close(simd_tail_l2, 3.0 * component_f64);
    assert_f64_close(scalar_cosine, 0.0);
    assert_f64_close(simd_cosine, 0.0);
    assert_f64_close(simd_tail_cosine, 0.0);
}

#[test]
fn should_preserve_wide_simd_lanes_for_distinct_vector_components() {
    // Arrange
    let component = 2.0e19_f32;
    let query = [
        component,
        component * 2.0,
        component * 3.0,
        component * 4.0,
        component * 5.0,
        component * 6.0,
        component * 7.0,
        component * 8.0,
    ];
    let target = [
        component * 8.0,
        component * 7.0,
        component * 6.0,
        component * 5.0,
        component * 4.0,
        component * 3.0,
        component * 2.0,
        component,
    ];
    let query_values = query.map(f64::from);
    let target_values = target.map(f64::from);
    let expected_dot = query_values
        .iter()
        .zip(target_values.iter())
        .map(|(left, right)| left * right)
        .sum::<f64>();
    let expected_l2 = query_values
        .iter()
        .zip(target_values.iter())
        .map(|(left, right)| (left - right).powi(2))
        .sum::<f64>()
        .sqrt();
    let query_norm = query_values.iter().map(|value| value.powi(2)).sum::<f64>();
    let target_norm = target_values.iter().map(|value| value.powi(2)).sum::<f64>();
    let expected_cosine = 1.0 - expected_dot / (query_norm.sqrt() * target_norm.sqrt());

    // Act
    let actual_dot = dot_score(&query, &target);
    let actual_l2 = l2_distance(&query, &target);
    let actual_cosine = cosine_distance(&query, &target);

    // Assert
    assert_f64_close(actual_dot, expected_dot);
    assert_f64_close(actual_l2, expected_l2);
    assert_f64_close(actual_cosine, expected_cosine);
}

#[test]
fn should_return_sentinel_values_for_mismatched_vector_lengths() {
    // Arrange
    let a = vec![1.0f32, 2.0, 3.0];
    let b = vec![1.0f32, 2.0];

    // Act
    let l2 = l2_distance(&a, &b);
    let cosine = cosine_distance(&a, &b);
    let dot = dot_score(&a, &b);

    // Assert
    assert_f64_close(l2, f64::MAX);
    assert_f64_close(cosine, 1.0);
    assert_f64_close(dot, 0.0);
}

#[test]
fn should_compute_hybrid_score_deterministically() {
    // Arrange
    let search_score = 0.2;
    let vector_score = 0.8;

    // Act
    let score = hybrid_score(search_score, vector_score, None);

    // Assert
    assert_f64_close(score, 0.41);
}

#[test]
fn should_hybrid_score_use_custom_weights() {
    // Arrange
    let search_score = 0.2;
    let vector_score = 0.8;
    let policy = cassie::hybrid::HybridScorePolicy {
        search_weight: 0.25,
        vector_weight: 0.75,
    };

    // Act
    let score = hybrid_score(search_score, vector_score, Some(&policy));

    // Assert
    assert_f64_close(score, 0.65);
}

#[test]
fn should_compute_tokenized_bm25_like_score_for_query_terms() {
    // Arrange
    let haystack = "The quick brown fox jumps over the lazy dog";
    let tokens = tokenizer::tokenize(haystack);

    // Act
    let tf_quick = usize_to_f64(tokens.iter().filter(|term| *term == "quick").count());
    let tf_dog = usize_to_f64(tokens.iter().filter(|term| *term == "dog").count());
    let dl = usize_to_f64(tokens.len());
    let avg_dl = dl;
    let k1 = 1.2;
    let b = 0.75;
    let n = 10.0;
    let score_common_term = bm25::bm25_score(tf_quick, 5.0, n, k1, b, dl, avg_dl);
    let score_rare_term = bm25::bm25_score(tf_dog, 1.0, n, k1, b, dl, avg_dl);
    let query_score = bm25::bm25_score(tf_quick, 5.0, n, k1, b, dl, avg_dl)
        + bm25::bm25_score(tf_dog, 1.0, n, k1, b, dl, avg_dl);
    let expected = score_common_term + score_rare_term;

    // Assert
    let observed = query_score;
    assert_f64_close(observed, expected);
    assert!(observed > score_common_term);
}

#[test]
fn should_clamp_bm25_score_for_invalid_document_frequency() {
    // Arrange
    let tf = 1.0;
    let df = 10.0;
    let n = 1.0;
    let k1 = 1.2;
    let b = 0.75;
    let dl = 3.0;
    let avg_dl = 3.0;

    // Act
    let score = bm25::bm25_score(tf, df, n, k1, b, dl, avg_dl);

    // Assert
    assert!(score.is_finite());
    assert!(score >= 0.0);
}

#[test]
fn should_generate_snippet_with_highlight_markup() {
    // Arrange
    let input = "Rust enables fast, reliable systems programming";
    let terms = vec!["rust".to_string(), "systems".to_string()];

    // Act
    let output = bm25::snippet(input, &terms, &AnalyzerConfig::default());

    // Assert
    assert_eq!(
        output,
        "<mark>Rust</mark> enables fast, reliable <mark>systems</mark> programming"
    );
}

#[test]
fn should_highlight_unicode_snippet_after_expanding_lowercase_character() {
    // Arrange
    let input = "İstanbul is nice";
    let terms = vec!["nice".to_string()];

    // Act
    let output = bm25::snippet(input, &terms, &AnalyzerConfig::default());

    // Assert
    assert_eq!(output, "İstanbul is <mark>nice</mark>");
}

#[test]
fn should_filter_stop_words_before_scoring_tokens() {
    // Arrange
    let input = "The quick brown fox and the lazy dog";

    // Act
    let tokens = tokenizer::tokenize(input);

    // Assert
    assert_eq!(tokens, vec!["quick", "brown", "fox", "lazy", "dog"]);
}
