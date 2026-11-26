use crate::benchmark::ExecutorKernel;

#[test]
fn should_keep_top_k_kernel_results_in_descending_score_order() {
    // Arrange
    let kernel = ExecutorKernel::sample();

    // Act
    let scores = kernel.top_k_scores();

    // Assert
    assert_eq!(scores, vec![9, 7, 4]);
}
