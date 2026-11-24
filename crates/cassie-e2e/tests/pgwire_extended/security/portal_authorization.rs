//! Current authority on retained materialized and cursor portal pages.

use super::support_portal_authorization::{
    assert_denied, assert_materialized_first_page, collect, Scenario,
};

#[test]
fn should_revalidate_connect_before_resuming_materialized_portal() {
    // Arrange
    let scenario = Scenario::MaterializedRevoke;
    // Act
    let transcript = collect(scenario);
    // Assert
    assert_materialized_first_page(&transcript.first);
    assert_denied(&transcript);
}

#[test]
fn should_reject_materialized_portal_for_dropped_login_role() {
    // Arrange
    let scenario = Scenario::MaterializedDrop;
    // Act
    let transcript = collect(scenario);
    // Assert
    assert_materialized_first_page(&transcript.first);
    assert_denied(&transcript);
}

#[test]
fn should_revalidate_connect_before_resuming_streaming_portal() {
    // Arrange
    let scenario = Scenario::StreamingRevoke;
    // Act
    let transcript = collect(scenario);
    // Assert
    assert_eq!(
        transcript.first.scan_entries, 2,
        "one row and cursor lookahead"
    );
    assert_denied(&transcript);
}
