//! Finite series overloads, descriptors, parameter identity and terminal errors.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_describe_series_native_overloads_without_retagging_supplied_parameters() {
    // Arrange
    let cases = [
        (0, "1", 23, 23, vec!["1", "2", "3"]),
        (23, "1", 23, 23, vec!["1", "2", "3"]),
        (20, "1", 20, 20, vec!["1", "2", "3"]),
        (1700, "1.00", 1700, 1700, vec!["1.00", "2.00", "3.00"]),
    ];
    let cycles = cases
        .iter()
        .map(|(oid, input, _, _, _)| {
            fixture::execute_cycle(
                "SELECT * FROM generate_series($1,3)",
                Some((*oid, 0, Some(input.as_bytes()))),
                0,
                true,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-808-series-parameter-oids", &[], cycles);

    // Assert
    for ((_, _, input_oid, output_oid, values), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        let width = match output_oid {
            23 => 4,
            20 => 8,
            _ => -1,
        };
        fixture::assert_single_column_descriptors(frames, (output_oid, width, -1), 0, true);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            vec![input_oid]
        );
        assert_eq!(
            wire::data_rows(frames),
            values
                .into_iter()
                .map(|value| vec![Some(value.to_owned())])
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn should_emit_series_rows_with_native_binary_column_widths() {
    // Arrange
    let sql = "SELECT value,position FROM generate_series(2,4,2) WITH ORDINALITY s(value,position)";
    let cycle = fixture::execute_cycle(sql, None, 1, true);

    // Act
    let batches = fixture::run_cycles("issue-808-series-binary", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    for (_, payload) in frames.iter().filter(|(tag, _)| *tag == b'T') {
        assert_eq!(
            wire::parse_row_description(payload)
                .iter()
                .map(|column| (column.name.as_str(), column.type_oid))
                .collect::<Vec<_>>(),
            vec![("value", 23), ("position", 20)]
        );
    }
    let expected = [(2_i32, 1_i64), (4, 2)].map(|(value, ordinal)| {
        let mut row = 2_i16.to_be_bytes().to_vec();
        row.extend_from_slice(&4_i32.to_be_bytes());
        row.extend_from_slice(&value.to_be_bytes());
        row.extend_from_slice(&8_i32.to_be_bytes());
        row.extend_from_slice(&ordinal.to_be_bytes());
        row
    });
    assert_eq!(fixture::data_row_payloads(frames), expected);
}

#[test]
fn should_reject_invalid_series_sources_without_partial_rows() {
    // Arrange
    let cases = [
        ("SELECT * FROM generate_series(NULL,NULL)","42725"),
        ("SELECT * FROM generate_series($1,(SELECT '3'))","42883"),
        ("SELECT * FROM generate_series(1::FLOAT8,2::FLOAT8)","42883"),
        ("SELECT * FROM generate_series(1,3,0)","22023"),
        ("SELECT * FROM generate_series((2147483647+1)-1,2147483647)","22003"),
        ("SELECT * FROM generate_series('NaN'::NUMERIC,3::NUMERIC)","22023"),
        ("SELECT * FROM generate_series(1::NUMERIC,'Infinity'::NUMERIC)","22023"),
        ("SELECT * FROM generate_series('2026-01-01'::TIMESTAMP,'2026-01-02'::TIMESTAMP,INTERVAL '1 month -30 days')","22023"),
        ("SELECT * FROM generate_series('2026-01-01'::TIMESTAMP,'2026-01-02'::TIMESTAMP,INTERVAL 'infinity')","22023"),
        ("SELECT * FROM generate_series('infinity'::TIMESTAMP,'infinity'::TIMESTAMP,INTERVAL '1 day') LIMIT 1","0A000"),
        ("SELECT * FROM generate_series('294276-12-31'::TIMESTAMP,'294276-12-31'::TIMESTAMP,INTERVAL '1 day') LIMIT 1","22008"),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(sql, _)| {
            [
                fixture::execute_cycle(sql, None, 0, true),
                fixture::execute_cycle("SELECT 8", None, 0, true),
            ]
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-808-series-error-recovery", &[], cycles);

    // Assert
    for ((sql, state), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0.iter()) {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some(state), "{sql}");
        assert_eq!(
            pair[0]
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'D' | b'C'))
                .count(),
            0,
            "{sql}"
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("8".to_owned())]]);
    }
}

#[test]
fn should_preserve_sql_routine_return_coercion_when_evaluating_series_arguments() {
    // Arrange
    let setup = [(
        r#"CREATE FUNCTION source_narrow(x SMALLINT) RETURNS SMALLINT IMMUTABLE AS "x + 1""#,
        vec![],
    )];
    let cycles = vec![
        fixture::execute_cycle(
            "SELECT * FROM generate_series(source_narrow(32767::SMALLINT),32768)",
            None,
            0,
            true,
        ),
        fixture::execute_cycle("SELECT 8", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("issue-808-source-routine-return-coercion", &setup, cycles);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    assert!(!batches[0]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'D' | b'C')));
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("8".to_owned())]]
    );
}

#[test]
fn should_infer_series_inputs_from_scalar_subquery_projection_identities() {
    // Arrange
    let cases = [
        ("SELECT * FROM generate_series($1,(SELECT 3))", 0, 23),
        ("SELECT * FROM generate_series((SELECT $1::BIGINT),(SELECT 3::BIGINT))", 20, 20),
        ("SELECT s.item FROM (VALUES (3)) v(n) CROSS JOIN LATERAL generate_series($1,(SELECT v.n)) s(item)", 0, 23),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, supplied, _)| {
            fixture::execute_cycle(sql, Some((*supplied, 0, Some(b"1"))), 0, true)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-808-series-subquery-parameters", &[], cycles);

    // Assert
    for ((sql, _, oid), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (oid, if oid == 23 { 4 } else { 8 }, -1),
            0,
            true,
        );
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            vec![oid],
            "{sql}"
        );
        assert_eq!(
            wire::data_rows(frames),
            vec![
                vec![Some("1".to_owned())],
                vec![Some("2".to_owned())],
                vec![Some("3".to_owned())]
            ],
            "{sql}"
        );
    }
}
