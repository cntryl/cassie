//! Stored CHAR-array logical values survive output modifiers and source boundaries.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn setup() -> [(&'static str, Vec<cassie::types::Value>); 2] {
    [
        (
            "CREATE TABLE logical_char_arrays (id INT PRIMARY KEY, items CHAR(3)[])",
            vec![],
        ),
        (
            "INSERT INTO logical_char_arrays VALUES (1,'[-2:-1]={x,NULL}'::CHAR(3)[])",
            vec![],
        ),
    ]
}

#[test]
fn should_publish_stored_character_array_padding_in_both_wire_formats() {
    // Arrange
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle("SELECT items FROM logical_char_arrays", None, format, true)
        })
        .to_vec();
    let mut binary = Vec::new();
    for value in [1_i32, 1, 1042, 2, -2, 3] {
        binary.extend_from_slice(&value.to_be_bytes());
    }
    binary.extend_from_slice(b"x  ");
    binary.extend_from_slice(&(-1_i32).to_be_bytes());

    // Act
    let batches = fixture::run_cycles("stored-character-array-output", &setup(), cycles);

    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[-2:-1]={\"x  \",NULL}".into())]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(&binary)]
    );
}

#[test]
fn should_preserve_logical_character_padding_after_array_function_modifiers_are_removed() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT array_reverse(items) FROM logical_char_arrays",
        None,
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("stored-character-array-function", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (1014, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[-2:-1]={NULL,\"x  \"}".into())]]
    );
}

#[test]
fn should_return_padded_stored_character_elements_from_unnest() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT item FROM logical_char_arrays CROSS JOIN LATERAL unnest(items) s(item)",
        None,
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("stored-character-array-unnest", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("x  ".into())], vec![None]]
    );
}

#[test]
fn should_hydrate_stored_character_arrays_before_returning_projection() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "UPDATE logical_char_arrays SET id=2 WHERE id=1 RETURNING items",
        None,
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("stored-character-array-returning", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[-2:-1]={\"x  \",NULL}".into())]]
    );
}

#[test]
fn should_preserve_character_padding_on_ordered_array_reads() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT items FROM logical_char_arrays ORDER BY id LIMIT 1",
        None,
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("stored-character-array-ordered", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[-2:-1]={\"x  \",NULL}".into())]]
    );
}
