//! PostgreSQL 18 money result identity, locale output and overflow tracked by #908.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

mod money_integer_division_rounding;
mod money_residuals;

#[test]
fn should_preserve_money_type_oids_through_sum() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 12::money AS amount, SUM(1::money) AS total",
    )])];

    // Act
    let batches = run_script("postgresql18-money-locale-and-sum", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![790, 790]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("$12.00".to_owned()), Some("$1.00".to_owned())]]
    );
}

#[test]
fn should_return_c_locale_money_text_over_postgresql_wire() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT '12.34'::money AS amount",
    )])];

    // Act
    let batches = run_script("postgresql18-money-c-locale-text", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (790, 8, -1)
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("$12.34".to_owned())]]
    );
}

#[test]
fn should_describe_typed_money_parameter_result() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "money_typed_parameter",
            "SELECT $1::money AS amount",
            &[790],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("money_typed_parameter"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-908-money-prepared-descriptor", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("money ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![790]);
    let result = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money RowDescription");
    let column = &wire::parse_row_description(&result.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (790, 8, -1)
    );
}

#[test]
fn should_reformat_prepared_money_after_monetary_locale_change() {
    // Arrange
    let statement = "prepared_money_locale_result";
    let sql = "SELECT amount FROM issue_908_prepared_money_locale";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET lc_monetary = 'C'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_908_prepared_money_locale (amount MONEY)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_908_prepared_money_locale VALUES ('1234.56'::MONEY)",
        )]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("money_c_locale_portal", statement, &[]),
            wire::describe_portal_frame("money_c_locale_portal"),
            wire::execute_frame("money_c_locale_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET lc_monetary = 'de_DE.UTF-8'",
        )]),
        Step::ThroughReady(vec![
            wire::bind_frame("money_german_locale_portal", statement, &[]),
            wire::describe_portal_frame("money_german_locale_portal"),
            wire::execute_frame("money_german_locale_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-prepared-money-locale-change", steps);

    // Assert
    for (index, batch) in batches.iter().enumerate() {
        assert_eq!(
            wire::error_code(batch),
            None,
            "batch {index} ErrorResponse fields: {:?}",
            batch
                .iter()
                .filter(|(tag, _)| *tag == b'E')
                .map(|(_, payload)| wire::parse_error_fields(payload))
                .collect::<Vec<_>>()
        );
    }
    for batch in [&batches[5], &batches[7]] {
        let description = batch
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("prepared money RowDescription");
        assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 790);
    }
    assert_eq!(
        wire::data_rows(&batches[5]),
        vec![vec![Some("$1,234.56".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[7]),
        vec![vec![Some("1.234,56 €".to_owned())]]
    );
}

#[test]
fn should_report_money_overflow_with_numeric_value_out_of_range_sqlstate() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 92233720368547758.08::numeric::money",
    )])];

    // Act
    let batches = run_script("postgresql18-money-overflow", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    assert!(!batches[0].iter().any(|(tag, _)| *tag == b'D'));
}

#[test]
fn should_round_money_casts_at_signed_half_cent_boundaries() {
    // Arrange
    let sql = "SELECT ((1.234::numeric)::money)::numeric, ((1.235::numeric)::money)::numeric, ((-1.235::numeric)::money)::numeric, ((-1.234::numeric)::money)::numeric";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-money-signed-rounding", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money rounding RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700, 1700, 1700]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("1.23".to_owned()),
            Some("1.24".to_owned()),
            Some("-1.24".to_owned()),
            Some("-1.23".to_owned()),
        ]]
    );
}

#[test]
fn should_check_money_signed_int64_lower_boundary() {
    // Arrange
    let upper_and_lower_endpoints = "SELECT ((92233720368547758.07::numeric)::money)::numeric, ((-92233720368547758.08::numeric)::money)::numeric";
    let below_lower_endpoint = "SELECT ((-92233720368547758.09::numeric)::money)::numeric";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(upper_and_lower_endpoints)]),
        Step::ThroughReady(vec![wire::simple_query_frame(below_lower_endpoint)]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("postgresql18-money-int64-endpoints", steps);

    // Assert
    assert_eq!(batches.len(), 3);
    let endpoints = &batches[0];
    assert_eq!(wire::error_code(endpoints), None);
    let description = endpoints
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money endpoint RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700]
    );
    assert_eq!(
        wire::data_rows(endpoints),
        vec![vec![
            Some("92233720368547758.07".to_owned()),
            Some("-92233720368547758.08".to_owned()),
        ]]
    );
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22003"));
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_send_money_values_in_postgresql_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let money_binary = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0xd2];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "money_binary_statement",
            "SELECT 12.34::numeric::money AS amount",
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "money_binary_portal",
                "money_binary_statement",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("money_binary_portal"),
            wire::execute_frame("money_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-908-money-binary-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money binary RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (790, 8, -1, 1)
    );
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![fixture::one_field_row(&money_binary)]
    );
}

#[test]
fn should_decode_binary_money_parameters_before_numeric_cast() {
    // Arrange
    let money_binary = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0xd2];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "money_binary_input",
            "SELECT $1::numeric AS amount",
            &[790],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "money_binary_input_portal",
                "money_binary_input",
                &[1],
                &[Some(&money_binary)],
                &[0],
            ),
            wire::describe_portal_frame("money_binary_input_portal"),
            wire::execute_frame("money_binary_input_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-908-money-binary-input", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money binary-input RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!((column.type_oid, column.format_code), (1_700, 0));
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("12.34".to_owned())]]
    );
}

#[test]
fn should_decode_text_money_parameters_before_numeric_cast() {
    // Arrange
    let money_text = b"$12.34";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "money_text_input",
            "SELECT $1::numeric AS amount",
            &[790],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "money_text_input_portal",
                "money_text_input",
                &[0],
                &[Some(money_text)],
                &[0],
            ),
            wire::describe_portal_frame("money_text_input_portal"),
            wire::execute_frame("money_text_input_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-908-money-text-input", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money text-input RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!((column.type_oid, column.format_code), (1_700, 0));
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("12.34".to_owned())]]
    );
}

#[test]
fn should_report_malformed_money_text_with_invalid_text_representation_sqlstate() {
    // PostgreSQL 18.6 returns 22P02 for this malformed money literal. Cassie
    // currently stops earlier with 0A000 because money is unsupported, so this
    // acceptance expectation is masked and does not prove input parsing.
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 'not-money'::money",
    )])];

    // Act
    let batches = run_script("issue-908-malformed-money-text", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        Some("22P02".to_owned()),
        "wire errors: {:?}",
        batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
}

#[test]
fn should_format_non_c_money_from_locale_specific_input() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET lc_monetary = 'de_DE.UTF-8'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT '1.234,56 €'::money AS parsed, (1234.56::numeric)::money AS cast_amount",
        )]),
    ];

    // Act
    let batches = run_script("issue-908-de-monetary-locale", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]), None);
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("German-locale money RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![790, 790]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("1.234,56 €".to_owned()),
            Some("1.234,56 €".to_owned()),
        ]]
    );
}

#[test]
fn should_describe_money_mixed_arithmetic_with_postgresql_result_oids() {
    // Arrange
    let sql = "SELECT '12.50'::money * 2 AS scaled, '12.50'::money / 2 AS divided, '12.50'::money / '6.25'::money AS ratio";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-908-money-mixed-arithmetic-types", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money arithmetic RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(790, 8, -1), (790, 8, -1), (701, 8, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("$25.00".to_owned()),
            Some("$6.25".to_owned()),
            Some("2".to_owned()),
        ]]
    );
}

#[test]
fn should_report_money_addition_overflow_sqlstate() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT '92233720368547758.07'::numeric::money + '0.01'::numeric::money",
    )])];

    // Act
    let batches = run_script("issue-908-money-arithmetic-boundaries", steps);

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    assert!(!batches[0].iter().any(|(tag, _)| *tag == b'D'));
}

#[test]
fn should_report_money_division_by_zero_sqlstate() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT '1.00'::money / 0",
    )])];

    // Act
    let batches = run_script("issue-908-money-division-by-zero", steps);

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22012"));
    assert!(!batches[0].iter().any(|(tag, _)| *tag == b'D'));
}

#[test]
fn should_recover_after_money_arithmetic_errors() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT '92233720368547758.07'::numeric::money + '0.01'::numeric::money",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("issue-908-money-arithmetic-error-recovery", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_send_negative_money_in_signed_postgresql_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let negative_money_binary = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfb, 0x2e];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "negative_money_binary_statement",
            "SELECT '-12.34'::money AS amount",
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "negative_money_binary_portal",
                "negative_money_binary_statement",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("negative_money_binary_portal"),
            wire::execute_frame("negative_money_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-908-negative-money-binary-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("negative money binary RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (790, 8, -1, 1)
    );
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![fixture::one_field_row(&negative_money_binary)]
    );
}

#[test]
fn should_decode_negative_binary_money_parameters_as_negative_c_locale_text() {
    // Arrange
    let negative_money_binary = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfb, 0x2e];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "negative_money_binary_input",
            "SELECT $1::text AS amount",
            &[790],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "negative_money_binary_input_portal",
                "negative_money_binary_input",
                &[1],
                &[Some(&negative_money_binary)],
                &[0],
            ),
            wire::describe_portal_frame("negative_money_binary_input_portal"),
            wire::execute_frame("negative_money_binary_input_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-908-negative-money-binary-input", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("-$12.34".to_owned())]]
    );
}

#[test]
fn should_report_money_sum_overflow_with_numeric_value_out_of_range_sqlstate() {
    // Arrange
    let overflow = "SELECT SUM(amount) FROM (VALUES ('92233720368547758.07'::money), ('0.01'::money)) AS valueset(amount)";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(overflow)]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 47")]),
    ];

    // Act
    let batches = run_script("issue-908-money-sum-overflow", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("47".to_owned())]]
    );
}
