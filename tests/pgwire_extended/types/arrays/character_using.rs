//! Existing NUMERIC USING targets consume logical CHAR-array inputs.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn using_result(label: &str, expression: &str) -> Vec<fixture::Frames> {
    let setup = [
        (
            "CREATE TABLE char_using(id INT PRIMARY KEY,items CHAR(3)[],amount TEXT)",
            vec![],
        ),
        (
            "INSERT INTO char_using VALUES(1,'[-2:-1]={x,NULL}'::CHAR(3)[],'0')",
            vec![],
        ),
    ];
    let sql =
        format!("ALTER TABLE char_using ALTER COLUMN amount TYPE NUMERIC(20,2) USING {expression}");
    fixture::run_cycles(
        label,
        &setup,
        vec![
            fixture::execute_cycle(&sql, None, 0, true),
            fixture::execute_cycle("SELECT amount FROM char_using", None, 0, true),
        ],
    )
}

#[test]
fn should_evaluate_numeric_using_from_logical_character_array_elements() {
    // Arrange
    let expression = "octet_length(items[-2])::NUMERIC";

    // Act
    let batches = using_result("character-array-using-input", expression);

    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
    }
    fixture::assert_single_column_descriptors(&batches[1], (1700, -1, 1_310_726), 0, true);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("3.00".into())]]
    );
}

#[test]
fn should_reapply_character_array_widths_inside_numeric_using() {
    // Arrange
    let expression = "octet_length((items::CHAR(4)[])[-2])::NUMERIC";

    // Act
    let batches = using_result("character-array-using-width", expression);

    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
    }
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("4.00".into())]]
    );
}

#[test]
fn should_preserve_character_source_identity_for_using_text_array_casts() {
    // Arrange
    let expression = "octet_length((items::TEXT[])[-2])::NUMERIC";

    // Act
    let batches = using_result("character-array-using-text-cast", expression);

    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
    }
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("1.00".into())]]
    );
}
