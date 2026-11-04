//! Cancellation must stop Boolean aggregate input before publishing output.

use crate::support_boolean_aggregate_cancellation::{
    assert_case, current_thread_runtime, run_case,
};

#[test]
fn should_cancel_boolean_aggregate_query_without_publishing_partial_output() {
    // Arrange
    let runtime = current_thread_runtime();

    // Act
    let case = runtime.block_on(run_case(&[], "(VALUES (TRUE), (FALSE)) AS inputs(flag)"));

    // Assert
    assert_case(&case, "worker-limit-one");
}

#[test]
fn should_cancel_native_boolean_aggregate_query_after_input_is_retained() {
    // Arrange
    let runtime = current_thread_runtime();
    let setup = [
        "CREATE TABLE issue_805_boolean_cancel (flag BOOLEAN)",
        "INSERT INTO issue_805_boolean_cancel VALUES (TRUE), (FALSE)",
    ];

    // Act
    let case = runtime.block_on(run_case(&setup, "issue_805_boolean_cancel"));

    // Assert
    assert_case(&case, "typed-worker-limit-one");
}
