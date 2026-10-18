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
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
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
fn should_preserve_numeric_aggregate_result_oids_over_wire() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let statements = [
        ("SELECT ABS($1) AS v", 700, 700, 4, false),
        ("SELECT SUM($1) AS v", 700, 700, 4, false),
        ("SELECT AVG($1) AS v", 1700, 1700, -1, false),
        ("SELECT COUNT($1) AS v", 1700, 20, 8, true),
    ];
    let inputs: [(Option<&[u8]>, Option<&str>); 3] = [
        (Some(b"2"), Some("2")),
        (Some(b"2.5"), Some("2.5")),
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
        let expected = expected_numeric_operation_row(result_oid, result_format, is_count, value);
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![expected],
            "{parameter_oid}:{sql}"
        );
    }
}

fn expected_numeric_operation_row(
    result_oid: i32,
    result_format: i16,
    is_count: bool,
    value: Option<&str>,
) -> Vec<u8> {
    use crate::support_type_metadata_contract as fixture;

    if is_count {
        let count = i64::from(value.is_some());
        return if result_format == 1 {
            fixture::one_field_row(&count.to_be_bytes())
        } else {
            fixture::one_field_row(count.to_string().as_bytes())
        };
    }

    let Some(value) = value else {
        return vec![0, 1, 255, 255, 255, 255];
    };
    if result_oid == 1700 {
        return if result_format == 1 {
            let numeric = match value {
                "2" => [0, 1, 0, 0, 0, 0, 0, 16, 0, 2].as_slice(),
                "2.5" => [0, 2, 0, 0, 0, 0, 0, 16, 0, 2, 19, 136].as_slice(),
                _ => unreachable!("numeric fixture value is known"),
            };
            fixture::one_field_row(numeric)
        } else {
            let text = match value {
                "2" => b"2.0000000000000000".as_slice(),
                "2.5" => b"2.5000000000000000".as_slice(),
                _ => unreachable!("numeric fixture value is known"),
            };
            fixture::one_field_row(text)
        };
    }

    if result_format == 1 {
        let float_value = value
            .parse::<f32>()
            .expect("fixture float value is exactly representable");
        fixture::one_field_row(&float_value.to_be_bytes())
    } else {
        fixture::one_field_row(value.as_bytes())
    }
}
