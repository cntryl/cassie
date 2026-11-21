use super::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;

fn frame_error(frame: &str) -> CassieError {
    let fixture = sql_fixture(
        "window-frame-validation",
        &["CREATE TABLE uf (id INT)", "INSERT INTO uf VALUES (1)"],
    );
    fixture.error(&format!("SELECT count(*) OVER ({frame}) FROM uf"))
}

fn assert_frame_error(error: &CassieError, message: &str) {
    assert_frame_error_with_state(error, "42P20", message);
}

fn assert_frame_error_with_state(error: &CassieError, sql_state: &str, message: &str) {
    assert_eq!(error.sql_state(), sql_state);
    assert!(
        error.to_string().contains(message),
        "error should mention '{message}'"
    );
}

fn assert_frame_accepted(frame: &str) {
    // Arrange
    let fixture = sql_fixture(
        "window-frame-accepted",
        &["CREATE TABLE uf (id INT)", "INSERT INTO uf VALUES (1)"],
    );

    // Act
    let result = fixture.execute(&format!("SELECT count(*) OVER ({frame}) FROM uf"));

    // Assert
    assert!(result.is_ok(), "frame {frame} should be accepted");
}

#[test]
fn should_reject_frame_starting_at_unbounded_following() {
    // Arrange
    let frame = "ROWS BETWEEN UNBOUNDED FOLLOWING AND UNBOUNDED FOLLOWING";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(&error, "frame start cannot be UNBOUNDED FOLLOWING");
}

#[test]
fn should_reject_unbounded_following_start_with_current_row_end() {
    // Arrange
    let frame = "ROWS BETWEEN UNBOUNDED FOLLOWING AND CURRENT ROW";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(&error, "frame start cannot be UNBOUNDED FOLLOWING");
}

#[test]
fn should_reject_short_form_unbounded_following() {
    // Arrange
    let frame = "ROWS UNBOUNDED FOLLOWING";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(&error, "frame start cannot be UNBOUNDED FOLLOWING");
}

#[test]
fn should_reject_frame_ending_at_unbounded_preceding() {
    // Arrange
    let frame = "ROWS BETWEEN CURRENT ROW AND UNBOUNDED PRECEDING";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(&error, "frame end cannot be UNBOUNDED PRECEDING");
}

#[test]
fn should_reject_unbounded_preceding_to_unbounded_preceding() {
    // Arrange
    let frame = "ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED PRECEDING";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(&error, "frame end cannot be UNBOUNDED PRECEDING");
}

#[test]
fn should_reject_offset_start_with_unbounded_preceding_end() {
    // Arrange
    let frame = "ROWS BETWEEN 1 PRECEDING AND UNBOUNDED PRECEDING";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(&error, "frame end cannot be UNBOUNDED PRECEDING");
}

#[test]
fn should_reject_current_row_start_with_preceding_offset_end() {
    // Arrange
    let frame = "ROWS BETWEEN CURRENT ROW AND 1 PRECEDING";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(
        &error,
        "frame starting from current row cannot have preceding rows",
    );
}

#[test]
fn should_reject_following_start_with_current_row_end() {
    // Arrange
    let frame = "ROWS BETWEEN 1 FOLLOWING AND CURRENT ROW";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(
        &error,
        "frame starting from following row cannot end with current row",
    );
}

#[test]
fn should_reject_following_start_with_preceding_end() {
    // Arrange
    let frame = "ROWS BETWEEN 1 FOLLOWING AND 1 PRECEDING";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(
        &error,
        "frame starting from following row cannot have preceding rows",
    );
}

#[test]
fn should_reject_short_form_following_offset() {
    // Arrange
    let frame = "ROWS 1 FOLLOWING";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error(
        &error,
        "frame starting from following row cannot end with current row",
    );
}

#[test]
fn should_reject_negative_frame_offset() {
    // Arrange
    let frame = "ROWS BETWEEN -1 PRECEDING AND CURRENT ROW";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error_with_state(
        &error,
        "22013",
        "frame starting offset must not be negative",
    );
}

#[test]
fn should_reject_null_frame_offset() {
    // Arrange
    let frame = "ROWS BETWEEN NULL PRECEDING AND CURRENT ROW";

    // Act
    let error = frame_error(frame);

    // Assert
    assert_frame_error_with_state(&error, "22004", "frame starting offset must not be null");
}

#[test]
fn should_accept_preceding_start_with_smaller_preceding_end() {
    assert_frame_accepted("ROWS BETWEEN 2 PRECEDING AND 3 PRECEDING");
}

#[test]
fn should_accept_following_start_with_smaller_following_end() {
    assert_frame_accepted("ROWS BETWEEN 3 FOLLOWING AND 2 FOLLOWING");
}

#[test]
fn should_accept_following_start_with_unbounded_following_end() {
    assert_frame_accepted("ROWS BETWEEN 1 FOLLOWING AND UNBOUNDED FOLLOWING");
}

#[test]
fn should_accept_short_form_preceding_offset() {
    assert_frame_accepted("ROWS 1 PRECEDING");
}
