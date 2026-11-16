use crate::support_pgwire as wire;

const READ_MODE: &str = "SELECT current_setting('cassie.projection_write_mode')";

fn extended_read_mode() -> Vec<Vec<u8>> {
    vec![
        wire::parse_frame("", READ_MODE),
        wire::bind_frame("", "", &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]
}

fn mode(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<Option<String>>> {
    wire::data_rows(frames)
}

fn declared() -> Vec<Vec<Option<String>>> {
    vec![vec![Some("replayable".into())]]
}

fn default_mode() -> Vec<Vec<Option<String>>> {
    vec![vec![Some("default".into())]]
}

#[test]
fn should_not_leak_a_declared_mode_to_the_next_transaction_on_a_reused_connection() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame("BEGIN")],
        vec![wire::simple_query_frame(
            "SET LOCAL cassie.projection_write_mode = 'replayable'",
        )],
        extended_read_mode(),
        vec![wire::simple_query_frame("COMMIT")],
        extended_read_mode(),
        vec![wire::simple_query_frame("BEGIN")],
        extended_read_mode(),
        vec![wire::simple_query_frame("ROLLBACK")],
        extended_read_mode(),
    ];

    // Act
    let frames = crate::support_repeatable_read::run_own_script(&[], script);

    // Assert
    for response in &frames {
        assert_eq!(wire::error_code(response), None);
    }
    assert_eq!(mode(&frames[2]), declared());
    assert_eq!(mode(&frames[4]), default_mode());
    assert_eq!(mode(&frames[6]), default_mode());
    assert_eq!(mode(&frames[8]), default_mode());
}

#[test]
fn should_clear_a_declared_mode_when_the_block_fails_and_rolls_back() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame("BEGIN")],
        vec![wire::simple_query_frame(
            "SET LOCAL cassie.projection_write_mode = 'replayable'",
        )],
        vec![wire::simple_query_frame("SELECT * FROM missing_projection")],
        vec![wire::simple_query_frame("ROLLBACK")],
        extended_read_mode(),
    ];

    // Act
    let frames = crate::support_repeatable_read::run_own_script_with_expected_errors(
        &[],
        script,
        &[(2, "42P01")],
    );

    // Assert
    assert_eq!(mode(&frames[4]), default_mode());
}

#[test]
fn should_reject_a_declaration_through_the_extended_protocol_outside_a_block() {
    // Arrange
    let script = vec![vec![
        wire::parse_frame("", "SET LOCAL cassie.projection_write_mode = 'replayable'"),
        wire::bind_frame("", "", &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]];

    // Act
    let frames = crate::support_repeatable_read::run_own_script_with_expected_errors(
        &[],
        script,
        &[(0, "25P01")],
    );

    // Assert
    assert_eq!(wire::error_code(&frames[0]).as_deref(), Some("25P01"));
}
