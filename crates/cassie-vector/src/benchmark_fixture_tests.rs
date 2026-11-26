use super::{brute_force, cosine_distance, dot_score, hnsw, l2_distance};

#[test]
fn should_compute_declared_distance_kernels_over_384_dimensions() {
    // Arrange
    let left = (0..384)
        .map(|value| f32::from(u16::try_from(value).expect("dimension fits u16")) / 384.0)
        .collect::<Vec<_>>();
    let right = (0..384)
        .map(|value| f32::from(u16::try_from(384 - value).expect("dimension fits u16")) / 384.0)
        .collect::<Vec<_>>();

    // Act
    let cosine = cosine_distance(&left, &right);
    let dot = dot_score(&left, &right);
    let l2 = l2_distance(&left, &right);

    // Assert
    assert_eq!(left.len(), 384);
    assert!(cosine.is_finite() && dot.is_finite() && l2.is_finite());
}

#[test]
fn should_select_top_k_candidates_from_the_owned_vector_algorithms() {
    // Arrange
    let query = [1.0, 0.0, 0.0];
    let candidates = vec![
        ("far".to_owned(), vec![0.0, 1.0, 0.0]),
        ("nearest".to_owned(), vec![1.0, 0.0, 0.0]),
        ("middle".to_owned(), vec![0.8, 0.2, 0.0]),
    ];

    // Act
    let exact = brute_force::top_k(&query, candidates.clone(), 2, l2_distance);
    let approximate = hnsw::search(&query, candidates, 2, l2_distance);

    // Assert
    assert_eq!(exact.len(), 2);
    assert_eq!(approximate.len(), 2);
    assert_eq!(exact[0].0, "nearest");
    assert_eq!(approximate[0].id, "nearest");
}
