use super::{run_script, wire, Step};

#[test]
fn should_round_fractional_numeric_when_scale_exceeds_precision() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('0.000994' AS NUMERIC(2, 5)) AS value",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('0.00100' AS NUMERIC(2, 5)) AS value",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovered")]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-scale-above-precision", steps);

    // Assert
    let rounded = &batches[0];
    assert_eq!(wire::error_code(rounded), None);
    let description = rounded
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("NUMERIC(2, 5) RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!((column.type_oid, column.type_mod), (1700, 131_081));
    assert_eq!(
        wire::data_rows(rounded),
        vec![vec![Some("0.00099".to_owned())]]
    );

    let overflow = &batches[1];
    assert_eq!(wire::error_code(overflow).as_deref(), Some("22003"));
    assert!(
        !overflow
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
        "overflow must not produce result metadata or rows"
    );
    assert_eq!(
        wire::error_code(&batches[2]),
        None,
        "a typmod overflow must leave the session usable"
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_apply_the_minimum_numeric_negative_scale() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1000, -1000)) AS value",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e999' AS NUMERIC(1000, -1000)) = 0::NUMERIC AS rounded_to_zero",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-minimum-scale", steps);

    // Assert
    let minimum_scale = &batches[0];
    assert_eq!(wire::error_code(minimum_scale), None);
    let description = minimum_scale
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("NUMERIC(1000, -1000) RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!((column.type_oid, column.type_mod), (1700, 65_537_052));
    assert_eq!(wire::data_rows(minimum_scale), vec![vec![None]]);

    let rounded = &batches[1];
    assert_eq!(wire::error_code(rounded), None);
    assert_eq!(
        wire::data_rows(rounded),
        vec![vec![Some("t".to_owned())]],
        "1e999 rounds to zero at scale -1000"
    );
}

#[test]
fn should_reject_numeric_scale_below_the_minimum() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1000, -1001)) AS value",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovered")]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-scale-below-minimum", steps);

    // Assert
    let below_minimum = &batches[0];
    assert_eq!(wire::error_code(below_minimum).as_deref(), Some("22023"));
    assert!(
        !below_minimum
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
        "invalid scale must not produce result metadata or rows"
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}
