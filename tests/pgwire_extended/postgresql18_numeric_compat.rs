//! PostgreSQL 18 NUMERIC OID, typmod and codec contracts tracked by #812.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

mod aggregate_special_values;
mod bigint_aggregate_overflow;
mod numeric_aggregate_fraction;
mod numeric_typmod_domain_edges;

#[test]
fn should_preserve_numeric_oid_typmod_text_binary_contract() {
    // Arrange
    let sql = "SELECT CAST($1 AS NUMERIC(5, 2))";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types("numeric_value", sql, &[1700])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("numeric_value"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("numeric_text_portal", "numeric_value", &["12.34"]),
            wire::describe_portal_frame("numeric_text_portal"),
            wire::execute_frame("numeric_text_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "numeric_binary_portal",
                "numeric_value",
                &[0],
                &[Some(b"12.34")],
                &[1],
            ),
            wire::describe_portal_frame("numeric_binary_portal"),
            wire::execute_frame("numeric_binary_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('10.00' AS NUMERIC(3, 2))",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-codecs", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let described = &batches[1];
    let parameter_types = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![1700]
    );
    let result_columns = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    let numeric_column = &wire::parse_row_description(&result_columns.1)[0];
    assert_eq!(numeric_column.type_oid, 1700);
    assert_eq!(numeric_column.type_mod, 327_686);

    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("12.34".to_owned())]]
    );
    let binary_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary RowDescription");
    let binary_column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(binary_column.type_oid, 1700);
    assert_eq!(binary_column.format_code, 1);
    let binary_data = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'D')
        .expect("binary DataRow");
    let mut expected = 1_i16.to_be_bytes().to_vec();
    expected.extend_from_slice(&12_i32.to_be_bytes());
    expected.extend_from_slice(&2_i16.to_be_bytes());
    expected.extend_from_slice(&0_i16.to_be_bytes());
    expected.extend_from_slice(&0_u16.to_be_bytes());
    expected.extend_from_slice(&2_i16.to_be_bytes());
    expected.extend_from_slice(&12_u16.to_be_bytes());
    expected.extend_from_slice(&3400_u16.to_be_bytes());
    assert_eq!(binary_data.1, expected);

    assert_eq!(wire::error_code(&batches[4]).as_deref(), Some("22003"));
}

#[test]
fn should_preserve_negative_scale_typmod_rounding_at_overflow() {
    // Arrange
    let sql = "SELECT CAST($1 AS NUMERIC(3, -2)) AS rounded_value";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "negative_numeric_scale",
            sql,
            &[1700],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("negative_numeric_scale"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "negative_numeric_positive_portal",
                "negative_numeric_scale",
                &["12345"],
            ),
            wire::describe_portal_frame("negative_numeric_positive_portal"),
            wire::execute_frame("negative_numeric_positive_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "negative_numeric_negative_portal",
                "negative_numeric_scale",
                &["-12345"],
            ),
            wire::describe_portal_frame("negative_numeric_negative_portal"),
            wire::execute_frame("negative_numeric_negative_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('99950' AS NUMERIC(3, -2))",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("postgresql18-negative-numeric-scale", steps);

    // Assert
    assert_eq!(
        batches[0].first().map(|(tag, _)| *tag),
        Some(b'1'),
        "Parse should accept NUMERIC(3, -2); SQLSTATE: {:?}",
        wire::error_code(&batches[0])
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::parse_parameter_description(
            &batches[1]
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("negative-scale ParameterDescription")
                .1
        ),
        vec![1700]
    );
    let statement_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("negative-scale statement RowDescription");
    let statement_column = &wire::parse_row_description(&statement_description.1)[0];
    assert_eq!(statement_column.type_oid, 1700);
    assert_eq!(statement_column.type_mod, 198_658);

    for (batch, expected) in [(&batches[2], "12300"), (&batches[3], "-12300")] {
        assert_eq!(wire::error_code(batch), None);
        let description = batch
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("negative-scale portal RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!((column.type_oid, column.type_mod), (1700, 198_658));
        assert_eq!(
            wire::data_rows(batch),
            vec![vec![Some(expected.to_owned())]]
        );
    }
    assert_eq!(wire::error_code(&batches[4]).as_deref(), Some("22003"));
    assert_eq!(
        wire::data_rows(&batches[4]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[5]), None);
    assert_eq!(
        wire::data_rows(&batches[5]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_describe_bigint_aggregate_results_as_numeric() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_aggregate_values (value BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_aggregate_values VALUES (9007199254740992), (9007199254740993)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT SUM(value), AVG(value) FROM issue_812_aggregate_values",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-bigint-aggregates", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[2]), None);
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700]
    );
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(-1, -1), (-1, -1)]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("18014398509481985".to_owned()),
            Some("9007199254740992.5000".to_owned())
        ]]
    );
}

#[test]
fn should_describe_bigint_aggregate_nulls_as_numeric() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_null_aggregate_values (value BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_null_aggregate_values VALUES (NULL)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT SUM(value), AVG(value) FROM issue_812_null_aggregate_values WHERE FALSE",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT SUM(value), AVG(value) FROM issue_812_null_aggregate_values",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-null-bigint-aggregates", steps);

    // Assert
    assert!(
        batches
            .iter()
            .all(|batch| wire::error_code(batch).is_none())
    );
    for batch in &batches[2..] {
        let description = batch
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("aggregate RowDescription");
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![1700, 1700]
        );
        assert_eq!(wire::data_rows(batch), vec![vec![None, None]]);
    }
}

#[test]
fn should_keep_grouped_bigint_aggregate_types_numeric() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_grouped_bigint_values (bucket INTEGER, value BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_grouped_bigint_values VALUES (1, 9007199254740992), (1, 9007199254740993), (2, 7), (2, 8)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE INDEX issue_812_grouped_bigint_bucket_value_idx ON issue_812_grouped_bigint_values (bucket, value)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT bucket, SUM(value), AVG(value) FROM issue_812_grouped_bigint_values WHERE bucket IN (1, 2) GROUP BY bucket ORDER BY bucket",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-grouped-bigint-aggregates", steps);

    // Assert
    assert!(
        batches
            .iter()
            .all(|batch| wire::error_code(batch).is_none())
    );
    let description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("grouped aggregate RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (1700, -1, -1), (1700, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![
            vec![
                Some("1".to_owned()),
                Some("18014398509481985".to_owned()),
                Some("9007199254740992.5000".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("15".to_owned()),
                Some("7.5000000000000000".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_encode_bigint_aggregate_results_as_numeric_in_binary_format() {
    // Arrange
    let statement = "bigint_aggregate_binary_results";
    let portal = "bigint_aggregate_binary_portal";
    let sql = "SELECT SUM(value), AVG(value) FROM issue_812_bigint_binary_aggregates";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_bigint_binary_aggregates (value BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_bigint_binary_aggregates VALUES (9007199254740992), (9007199254740993)",
        )]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-bigint-aggregate-binary-results", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(wire::error_code(&batches[3]), None);
    assert_eq!(wire::error_code(&batches[4]), None);
    let statement_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared aggregate RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    let statement_contract = statement_columns
        .iter()
        .map(|column| {
            (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            )
        })
        .collect::<Vec<_>>();

    let binary_description = batches[4]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary aggregate portal RowDescription");
    let binary_columns = wire::parse_row_description(&binary_description.1);
    let binary_contract = binary_columns
        .iter()
        .map(|column| {
            (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            )
        })
        .collect::<Vec<_>>();
    let sum = [
        0x00, 0x05, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x1f, 0x4e, 0x0f, 0x91, 0x03,
        0xb4, 0x07, 0xc1,
    ];
    let average = [
        0x00, 0x05, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x23, 0x2f, 0x07, 0xc8, 0x15, 0x62, 0x03,
        0xe0, 0x13, 0x88,
    ];
    let mut expected_row = vec![0x00, 0x02];
    for value in [&sum[..], &average[..]] {
        expected_row.extend_from_slice(
            &i32::try_from(value.len())
                .expect("numeric wire value length fits i32")
                .to_be_bytes(),
        );
        expected_row.extend_from_slice(value);
    }
    let binary_rows = crate::support_type_metadata_contract::data_row_payloads(&batches[4]);
    assert_eq!(
        (statement_contract, binary_contract, binary_rows),
        (
            vec![(1700, -1, -1, 0), (1700, -1, -1, 0)],
            vec![(1700, -1, -1, 1), (1700, -1, -1, 1)],
            vec![expected_row],
        ),
        "prepared and binary portal metadata with SUM/AVG DataRow bytes"
    );
}

#[test]
fn should_preserve_exact_numeric_aggregate_values_beyond_float_precision() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_numeric_aggregate_values (value NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_numeric_aggregate_values VALUES (9007199254740993.1255), (0.0001)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT SUM(value), AVG(value) FROM issue_812_numeric_aggregate_values",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-aggregate-precision", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[2]), None);
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700],
        "aggregate values: {:?}",
        wire::data_rows(&batches[2])
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("9007199254740993.1256".to_owned()),
            Some("4503599627370496.5628".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_numeric_aggregates_beyond_thirty_eight_digits() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_wide_numeric_aggregates (value NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_wide_numeric_aggregates VALUES \
             ('123456789012345678901234567890123456789012345678.90'), (0.11)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT SUM(value), AVG(value), MAX(value) + 0.11, MAX(value) * 2 \
             FROM issue_812_wide_numeric_aggregates",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-wide-numeric-aggregates", steps);

    // Assert
    assert!(
        batches
            .iter()
            .all(|batch| wire::error_code(batch).is_none())
    );
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("wide numeric aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(1700, -1, -1); 4]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("123456789012345678901234567890123456789012345679.01".to_owned()),
            Some("61728394506172839450617283945061728394506172839.51".to_owned()),
            Some("123456789012345678901234567890123456789012345679.01".to_owned()),
            Some("246913578024691357802469135780246913578024691357.80".to_owned()),
        ]]
    );
}

#[test]
fn should_decode_binary_numeric_values_beyond_thirty_eight_digits() {
    // Arrange
    let value = [
        0x00, 0x12, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x14, 0x00, 0x0c, 0x0d, 0x80, 0x1e, 0xd2, 0x04,
        0xd2, 0x16, 0x2e, 0x23, 0x34, 0x0d, 0x80, 0x1e, 0xd2, 0x04, 0xd2, 0x16, 0x2e, 0x23, 0x34,
        0x0d, 0x80, 0x1e, 0xd2, 0x04, 0xd2, 0x16, 0x2e, 0x23, 0x34, 0x0d, 0x80, 0x1e, 0xd2,
    ];
    let sql = "SELECT $1::NUMERIC AS amount";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types("wide_numeric", sql, &[1700])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("wide_numeric"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "wide_numeric_portal",
                "wide_numeric",
                &[1],
                &[Some(&value)],
                &[0],
            ),
            wire::describe_portal_frame("wide_numeric_portal"),
            wire::execute_frame("wide_numeric_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-wide-binary-input", steps);

    // Assert
    assert_eq!(
        wire::parse_parameter_description(
            &batches[1]
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("wide numeric ParameterDescription")
                .1
        ),
        vec![1700]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("wide numeric RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(column.type_oid, 1700);
    assert_eq!(column.type_mod, -1);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some(
            "12345678901234567890123456789012345678901234567890.12345678901234567890".to_owned()
        )]]
    );
}

#[test]
fn should_preserve_wide_numeric_oid_zero_binary_round_trip() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let value = [
        0x00, 0x12, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x14, 0x00, 0x0c, 0x0d, 0x80, 0x1e, 0xd2, 0x04,
        0xd2, 0x16, 0x2e, 0x23, 0x34, 0x0d, 0x80, 0x1e, 0xd2, 0x04, 0xd2, 0x16, 0x2e, 0x23, 0x34,
        0x0d, 0x80, 0x1e, 0xd2, 0x04, 0xd2, 0x16, 0x2e, 0x23, 0x34, 0x0d, 0x80, 0x1e, 0xd2,
    ];
    let statement = "wide_oid_zero_numeric";
    let portal = "wide_oid_zero_numeric_portal";
    let sql = "SELECT CAST($1 AS NUMERIC) AS amount";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[0])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[1], &[Some(&value[..])], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-wide-numeric-oid-zero-binary", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let statement_description = &batches[1];
    assert_eq!(wire::error_code(statement_description), None);
    let parameter_types = statement_description
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("wide numeric ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![1700]
    );
    let statement_row_description = statement_description
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("wide numeric statement RowDescription");
    let statement_column = &wire::parse_row_description(&statement_row_description.1)[0];
    assert_eq!(
        (statement_column.type_oid, statement_column.type_size),
        (1700, -1)
    );

    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    fixture::assert_single_column_descriptors(result, (1700, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![fixture::one_field_row(&value)]
    );
}

#[test]
fn should_reject_numeric_exponents_beyond_postgresql_integer_digit_limit() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e131072' AS NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-absolute-range", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_enforce_postgresql_numeric_precision_scale_limits() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1000, 0))",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1000, 1000))",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e-16383' AS NUMERIC) > 0",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e131071' AS NUMERIC) > 0",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e-16384' AS NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('1e131072' AS NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1001, 0))",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST(NULL AS NUMERIC(1000, 1001))",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-precision-scale-limits", steps);

    // Assert
    for (index, batch) in batches.iter().take(4).enumerate() {
        assert_eq!(wire::error_code(batch), None, "valid boundary {index}");
    }
    for (index, expected_state) in [(4, "22003"), (5, "22003"), (6, "22023"), (7, "22023")] {
        assert_eq!(
            wire::error_code(&batches[index]).as_deref(),
            Some(expected_state),
            "rejection boundary {index}"
        );
    }
    for (index, expected_typmod) in [(0, 65_536_004), (1, 65_537_004)] {
        let description = batches[index]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("numeric boundary RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!(column.type_oid, 1700);
        assert_eq!(column.type_mod, expected_typmod);
        assert_eq!(wire::data_rows(&batches[index]), vec![vec![None]]);
    }
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("t".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("t".to_owned())]]
    );
}

#[test]
fn should_preserve_numeric_aggregate_result_oids_over_wire() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let statements = [
        ("SELECT ABS($1) AS v", 700, 700, 4, false),
        ("SELECT SUM($1) AS v", 700, 700, 4, false),
        ("SELECT AVG($1) AS v", 1700, 1700, -1, false),
        ("SELECT COUNT($1) AS v", 1700, 20, 8, true),
    ];
    let inputs: [(Option<&[u8]>, Option<f64>); 3] = [
        (Some(b"2"), Some(2.0)),
        (Some(b"2.5"), Some(2.5)),
        (None, None),
    ];
    let mut cases = Vec::new();
    let mut cycles = Vec::new();
    for (sql, parameter_oid, result_oid, result_size, is_count) in statements {
        for result_format in [0, 1] {
            for (input, value) in inputs {
                cycles.push(fixture::execute_cycle(
                    sql,
                    Some((parameter_oid, 0, input)),
                    result_format,
                    true,
                ));
                cases.push((
                    sql,
                    parameter_oid,
                    result_oid,
                    result_size,
                    result_format,
                    value,
                    is_count,
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("postgresql18-numeric-operation-result-types", &[], cycles);

    // Assert
    for ((sql, parameter_oid, result_oid, result_size, result_format, value, is_count), frames) in
        cases.into_iter().zip(batches)
    {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(
            &frames,
            (result_oid, result_size, -1),
            result_format,
            true,
        );
        let parameter_description = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("operation parameter description");
        assert_eq!(
            wire::parse_parameter_description(&parameter_description.1),
            vec![parameter_oid],
            "{parameter_oid}:{sql} parameter OID"
        );
        let expected = if is_count {
            let count = i64::from(value.is_some());
            if result_format == 1 {
                fixture::one_field_row(&count.to_be_bytes())
            } else {
                fixture::one_field_row(count.to_string().as_bytes())
            }
        } else if result_oid == 1700 {
            value.map_or_else(
                || vec![0, 1, 255, 255, 255, 255],
                |value| {
                    if result_format == 1 {
                        let numeric = if value == 2.0 {
                            [0, 1, 0, 0, 0, 0, 0, 16, 0, 2].as_slice()
                        } else {
                            [0, 2, 0, 0, 0, 0, 0, 16, 0, 2, 19, 136].as_slice()
                        };
                        fixture::one_field_row(numeric)
                    } else {
                        let text = if value == 2.0 {
                            b"2.0000000000000000".as_slice()
                        } else {
                            b"2.5000000000000000".as_slice()
                        };
                        fixture::one_field_row(text)
                    }
                },
            )
        } else {
            value.map_or_else(
                || vec![0, 1, 255, 255, 255, 255],
                |value| {
                    if result_format == 1 {
                        fixture::one_field_row(&(value as f32).to_be_bytes())
                    } else {
                        fixture::one_field_row(value.to_string().as_bytes())
                    }
                },
            )
        };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![expected],
            "{parameter_oid}:{sql}"
        );
    }
}

#[test]
fn should_bind_an_oid_zero_numeric_parameter_in_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let value = [
        0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x13, 0x88,
    ];
    let sql = "SELECT CAST($1 AS NUMERIC) AS value";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types("oid_zero_numeric", sql, &[0])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("oid_zero_numeric"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "oid_zero_numeric_portal",
                "oid_zero_numeric",
                &[1],
                &[Some(&value)],
                &[1],
            ),
            wire::describe_portal_frame("oid_zero_numeric_portal"),
            wire::execute_frame("oid_zero_numeric_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-oid-zero-binary-bind", steps);

    // Assert
    let parameters = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("numeric ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![1700]);
    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    fixture::assert_single_column_descriptors(result, (1700, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![fixture::one_field_row(&value)]
    );
}

#[test]
fn should_encode_numeric_special_values_in_binary() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let text_sql = "SELECT 'NaN'::NUMERIC AS nan_value, 'Infinity'::NUMERIC AS positive_infinity, '-Infinity'::NUMERIC AS negative_infinity, 'NaN'::NUMERIC = 'NaN'::NUMERIC AS nan_equal, 'Infinity'::NUMERIC > 999999::NUMERIC AS positive_infinity_orders_last, '-Infinity'::NUMERIC < -999999::NUMERIC AS negative_infinity_orders_first";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(text_sql)]),
        Step::ParseFlush(wire::parse_frame(
            "numeric_nan_binary_statement",
            "SELECT 'NaN'::NUMERIC AS value",
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "numeric_nan_binary_portal",
                "numeric_nan_binary_statement",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("numeric_nan_binary_portal"),
            wire::execute_frame("numeric_nan_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-812-numeric-special-values", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let text_description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric special-value RowDescription");
    assert_eq!(
        wire::parse_row_description(&text_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700, 1700, 16, 16, 16]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("NaN".to_owned()),
            Some("Infinity".to_owned()),
            Some("-Infinity".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
        ]]
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    assert_eq!(wire::error_code(&batches[2]), None);
    let binary_description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary numeric special-value RowDescription");
    let column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (1700, -1, -1, 1)
    );
    let numeric_nan_binary = [0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00];
    assert_eq!(
        fixture::data_row_payloads(&batches[2]),
        vec![fixture::one_field_row(&numeric_nan_binary)]
    );
}

#[test]
fn should_encode_both_numeric_infinity_signs_in_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let positive_infinity = [0x00, 0x00, 0x00, 0x00, 0xd0, 0x00, 0x00, 0x20];
    let negative_infinity = [0x00, 0x00, 0x00, 0x00, 0xf0, 0x00, 0x00, 0x20];
    let cases = [
        (
            "numeric_positive_infinity",
            "SELECT 'Infinity'::NUMERIC AS value",
            &positive_infinity[..],
        ),
        (
            "numeric_negative_infinity",
            "SELECT '-Infinity'::NUMERIC AS value",
            &negative_infinity[..],
        ),
    ];
    let mut steps = Vec::new();
    for (index, (statement, sql, _)) in cases.iter().enumerate() {
        let portal = format!("numeric_infinity_portal_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame(statement, sql)));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(&portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(&portal),
            wire::execute_frame(&portal),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("issue-812-numeric-infinity-binary", steps);

    // Assert
    for (index, (statement, _, expected)) in cases.into_iter().enumerate() {
        let result = &batches[index * 2 + 1];
        assert_eq!(wire::error_code(result), None, "{statement}");
        let description = result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("numeric infinity RowDescription");
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!((column.type_oid, column.format_code), (1700, 1));
        assert_eq!(
            fixture::data_row_payloads(result),
            vec![fixture::one_field_row(expected)],
            "{statement} binary payload"
        );
    }
}

#[test]
fn should_decode_numeric_infinity_binary_parameters() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let positive_infinity = [0x00, 0x00, 0x00, 0x00, 0xd0, 0x00, 0x00, 0x20];
    let negative_infinity = [0x00, 0x00, 0x00, 0x00, 0xf0, 0x00, 0x00, 0x20];
    let cases = [
        (
            "numeric_positive_infinity_input",
            &positive_infinity[..],
            "Infinity",
        ),
        (
            "numeric_negative_infinity_input",
            &negative_infinity[..],
            "-Infinity",
        ),
    ];
    let mut steps = Vec::new();
    for (statement, payload, _) in &cases {
        let portal = format!("{statement}_portal");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1 AS value",
            &[1_700],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(&portal, statement, &[1], &[Some(*payload)], &[0]),
            wire::describe_portal_frame(&portal),
            wire::execute_frame(&portal),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("issue-812-numeric-infinity-binary-input", steps);

    // Assert
    for (index, (statement, _, expected)) in cases.into_iter().enumerate() {
        let result = &batches[index * 2 + 1];
        assert_eq!(wire::error_code(result), None, "{statement}");
        fixture::assert_single_column_descriptors(result, (1_700, -1, -1), 0, true);
        assert_eq!(
            wire::data_rows(result),
            vec![vec![Some(expected.to_owned())]],
            "{statement} text value"
        );
    }
}

#[test]
fn should_report_numeric_bind_input_errors_with_postgresql_sqlstates() {
    // Arrange
    let statement_cases = [
        (
            "numeric_bad_text_input",
            "numeric_bad_text_portal",
            0,
            &b"not-a-number"[..],
            "22P02",
        ),
        (
            "numeric_bad_binary_digit",
            "numeric_bad_binary_digit_portal",
            1,
            &[0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x27, 0x10][..],
            "22P03",
        ),
        (
            "numeric_short_binary_header",
            "numeric_short_binary_header_portal",
            1,
            &[0x00, 0x01][..],
            "08P01",
        ),
    ];
    let mut steps = Vec::new();
    for (statement, portal, parameter_format, value, _) in statement_cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1::NUMERIC AS amount",
            &[1700],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                portal,
                statement,
                &[parameter_format],
                &[Some(value)],
                &[0],
            ),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42 AS recovered",
        )]));
    }

    // Act
    let batches = run_script("postgresql18-numeric-input-error-sqlstates", steps);

    // Assert
    assert_eq!(batches.len(), statement_cases.len() * 4);
    for (index, (statement, _, _, _, expected_state)) in statement_cases.iter().enumerate() {
        let offset = index * 4;
        assert_eq!(
            wire::error_code(&batches[offset]),
            None,
            "{statement} Parse"
        );
        let parameter_description = batches[offset + 1]
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("numeric ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameter_description.1),
            vec![1700],
            "{statement} parameter OID"
        );
        let error_batch = &batches[offset + 2];
        assert_eq!(
            wire::error_code(error_batch).as_deref(),
            Some(*expected_state),
            "{statement} wire error fields: {:?}",
            error_batch
                .iter()
                .filter(|(tag, _)| *tag == b'E')
                .map(|(_, payload)| wire::parse_error_fields(payload))
                .collect::<Vec<_>>()
        );
        assert!(
            !error_batch
                .iter()
                .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
        );
        assert_eq!(
            error_batch.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..]),
            "{statement} should return ReadyForQuery after Bind error"
        );
        assert_eq!(wire::error_code(&batches[offset + 3]), None);
        assert_eq!(
            wire::data_rows(&batches[offset + 3]),
            vec![vec![Some("42".to_owned())]],
            "{statement} session recovery"
        );
    }
}
