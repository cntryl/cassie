use crate::support_pgwire as wire;

#[test]
fn should_publish_transactional_copy_in_replayable_projector_transaction() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame("BEGIN")],
        vec![wire::simple_query_frame(
            "SET LOCAL cassie.projection_write_mode = 'replayable'",
        )],
        vec![
            wire::simple_query_frame("COPY projector_copy (n) FROM STDIN"),
            wire::copy_data_frame(b"41\n"),
            wire::copy_done_frame(),
        ],
        vec![wire::simple_query_frame(
            "INSERT INTO projector_checkpoint (position) VALUES (1)",
        )],
        vec![wire::simple_query_frame("COMMIT")],
        vec![wire::simple_query_frame("SELECT n FROM projector_copy")],
        vec![wire::simple_query_frame(
            "SELECT position FROM projector_checkpoint",
        )],
    ];

    // Act
    let frames = crate::support_repeatable_read::run_own_script(
        &[
            "CREATE TABLE projector_copy (n INT)",
            "CREATE TABLE projector_checkpoint (position INT)",
        ],
        script,
    );

    // Assert
    for response in &frames {
        assert_eq!(wire::error_code(response), None);
    }
    assert_eq!(wire::data_rows(&frames[5]), vec![vec![Some("41".into())]]);
    assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("1".into())]]);
}

#[test]
fn should_discard_projector_copy_rows_after_savepoint_rollback() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame("BEGIN")],
        vec![wire::simple_query_frame(
            "SET LOCAL cassie.projection_write_mode = 'replayable'",
        )],
        vec![wire::simple_query_frame("SAVEPOINT before_copy")],
        vec![
            wire::simple_query_frame("COPY projector_copy_rollback (n) FROM STDIN"),
            wire::copy_data_frame(b"41\n"),
            wire::copy_done_frame(),
        ],
        vec![wire::simple_query_frame("ROLLBACK TO before_copy")],
        vec![wire::simple_query_frame("COMMIT")],
        vec![wire::simple_query_frame(
            "SELECT count(*) FROM projector_copy_rollback",
        )],
    ];

    // Act
    let frames = crate::support_repeatable_read::run_own_script(
        &["CREATE TABLE projector_copy_rollback (n INT)"],
        script,
    );

    // Assert
    for response in &frames {
        assert_eq!(wire::error_code(response), None);
    }
    assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("0".into())]]);
}

#[test]
fn should_execute_copy_to_sequence_effect_once_in_projector_transaction() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame("BEGIN")],
        vec![wire::simple_query_frame(
            "SET LOCAL cassie.projection_write_mode = 'replayable'",
        )],
        vec![wire::simple_query_frame(
            "COPY (SELECT nextval('projector_copy_sequence')) TO STDOUT",
        )],
        vec![wire::simple_query_frame(
            "INSERT INTO projector_copy_checkpoint (position) VALUES (1)",
        )],
        vec![wire::simple_query_frame("COMMIT")],
        vec![wire::simple_query_frame(
            "SELECT currval('projector_copy_sequence')",
        )],
    ];

    // Act
    let frames = crate::support_repeatable_read::run_own_script(
        &[
            "CREATE SEQUENCE projector_copy_sequence",
            "CREATE TABLE projector_copy_checkpoint (position INT)",
        ],
        script,
    );

    // Assert
    for response in &frames {
        assert_eq!(wire::error_code(response), None);
    }
    assert_eq!(wire::data_rows(&frames[5]), vec![vec![Some("1".into())]]);
}
