use super::{configured_cassie, extended_text_round_trip, simple_round_trip, support};

#[test]
fn should_preserve_duplicate_expression_labels_by_position_on_wire() {
    // Arrange
    let (simple_cassie, simple_path) = configured_cassie("expression-labels-simple", &[]);
    let (extended_cassie, extended_path) = configured_cassie("expression-labels-extended", &[]);
    let sql = "SELECT 1 + 1, 2 + 2, CAST(3 AS int)";

    // Act
    let simple = simple_round_trip(simple_cassie, simple_path, sql);
    let extended = extended_text_round_trip(extended_cassie, extended_path, sql);

    // Assert
    for frames in [&simple, &extended] {
        assert_eq!(support::error_code(frames), None);
        let description = frames
            .iter()
            .find(|frame| frame.0 == b'T')
            .map(|frame| support::parse_row_description(&frame.1))
            .expect("RowDescription");
        assert_eq!(
            description
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["?column?", "?column?", "int4"]
        );
        assert_eq!(
            support::data_rows(frames),
            vec![vec![
                Some("2".to_owned()),
                Some("4".to_owned()),
                Some("3".to_owned()),
            ]]
        );
    }
}
