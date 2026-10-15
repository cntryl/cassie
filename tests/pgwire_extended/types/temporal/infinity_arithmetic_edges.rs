//! PostgreSQL 18.6 infinity ordering and arithmetic edges for #814.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_order_all_temporal_infinities() {
    // Arrange
    let sql = concat!(
        "SELECT DATE 'infinity' > DATE '5874897-12-31' AS date_positive_orders_last, ",
        "DATE '-infinity' < DATE '4713-01-01 BC' AS date_negative_orders_first, ",
        "TIMESTAMP 'infinity' > TIMESTAMP '294276-12-31 23:59:59.999999' AS timestamp_positive_orders_last, ",
        "TIMESTAMP '-infinity' < TIMESTAMP '4713-01-01 00:00:00 BC' AS timestamp_negative_orders_first, ",
        "TIMESTAMPTZ 'infinity' > TIMESTAMPTZ '294276-12-31 23:59:59.999999+00' AS timestamptz_positive_orders_last, ",
        "TIMESTAMPTZ '-infinity' < TIMESTAMPTZ '4713-01-01 00:00:00+00 BC' AS timestamptz_negative_orders_first"
    );
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
    ];

    // Act
    let batches = run_script("issue-814-temporal-infinity-ordering", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("temporal infinity comparison RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 6]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("t".to_owned()); 6]]
    );
}

#[test]
fn should_preserve_temporal_infinities_under_finite_arithmetic() {
    // Arrange
    let sql = concat!(
        "SELECT DATE 'infinity' + 1 = DATE 'infinity' AS date_positive_survives_addition, ",
        "DATE '-infinity' - 1 = DATE '-infinity' AS date_negative_survives_subtraction, ",
        "TIMESTAMP 'infinity' + INTERVAL '1 day' = TIMESTAMP 'infinity' AS timestamp_positive_survives_addition, ",
        "TIMESTAMP '-infinity' - INTERVAL '1 day' = TIMESTAMP '-infinity' AS timestamp_negative_survives_subtraction, ",
        "TIMESTAMPTZ 'infinity' - INTERVAL '1 day' = TIMESTAMPTZ 'infinity' AS timestamptz_positive_survives_subtraction, ",
        "TIMESTAMPTZ '-infinity' + INTERVAL '1 day' = TIMESTAMPTZ '-infinity' AS timestamptz_negative_survives_addition"
    );
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
    ];

    // Act
    let batches = run_script("issue-814-temporal-infinity-finite-arithmetic", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("temporal infinity arithmetic RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 6]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("t".to_owned()); 6]]
    );
}

#[test]
fn should_report_datetime_22008_for_temporal_range_errors() {
    // Arrange
    let cases = [
        (
            "date_finite_upper_bound",
            "SELECT DATE '5874897-12-31' + 1",
            "date out of range",
        ),
        (
            "timestamp_finite_upper_bound",
            "SELECT TIMESTAMP '294276-12-31 23:59:59.999999' + INTERVAL '1 microsecond'",
            "timestamp out of range",
        ),
        (
            "timestamptz_finite_upper_bound",
            "SELECT TIMESTAMPTZ '294276-12-31 23:59:59.999999+00' + INTERVAL '1 microsecond'",
            "timestamp out of range",
        ),
        (
            "date_matching_infinities",
            "SELECT DATE 'infinity' - DATE 'infinity'",
            "cannot subtract infinite dates",
        ),
        (
            "timestamp_matching_infinities",
            "SELECT TIMESTAMP 'infinity' - TIMESTAMP 'infinity'",
            "interval out of range",
        ),
        (
            "timestamptz_matching_infinities",
            "SELECT TIMESTAMPTZ 'infinity' - TIMESTAMPTZ 'infinity'",
            "interval out of range",
        ),
    ];
    let mut steps = Vec::with_capacity(cases.len() * 2);
    for (_, sql, _) in &cases {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 1",
        )]));
    }

    // Act
    let batches = run_script("issue-814-temporal-range-arithmetic-errors", steps);

    // Assert
    for (index, (case, _, expected_message)) in cases.into_iter().enumerate() {
        let result = &batches[index * 2];
        assert_eq!(
            wire::error_code(result).as_deref(),
            Some("22008"),
            "{case}: {:?}",
            result
                .iter()
                .filter(|(tag, _)| *tag == b'E')
                .map(|(_, payload)| wire::parse_error_fields(payload))
                .collect::<Vec<_>>()
        );
        assert!(
            result.iter().any(|(tag, payload)| {
                *tag == b'E'
                    && wire::parse_error_fields(payload)
                        .iter()
                        .any(|(kind, message)| *kind == 'M' && message == expected_message)
            }),
            "{case}: PostgreSQL reports `{expected_message}`"
        );
        assert_eq!(wire::error_code(&batches[index * 2 + 1]), None);
        assert_eq!(
            wire::data_rows(&batches[index * 2 + 1]),
            vec![vec![Some("1".to_owned())]],
            "connection should recover after {case}"
        );
    }
}
