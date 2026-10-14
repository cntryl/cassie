//! Declared scalar-array text input and unlimited VARCHAR contracts.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;
use cassie::types::Value;

type IntegerArrayEndpoint = (
    i32,
    &'static [u8],
    &'static [u8],
    &'static [u8],
    &'static [u8],
);

#[test]
fn should_infer_integer_array_constructor_type_for_null_elements() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT ARRAY[NULL, 1] AS values_array", None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-constructor-null-element", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{NULL,1}".to_owned())]]
    );
}

#[test]
fn should_reject_scalar_array_text_elements_outside_the_declared_domain() {
    // Arrange
    let cases: [(i32, &[u8]); 26] = [
        (1005, b"{32768}"),
        (1005, b"[32768]"),
        (1007, b"{-2147483649}"),
        (1007, b"[-2147483649]"),
        (1016, b"{9223372036854775808}"),
        (1016, b"[9223372036854775808]"),
        (1016, b"[1.5]"),
        (1005, b"[true]"),
        (1000, b"{not-a-boolean}"),
        (1000, b"[1]"),
        (1022, b"{NaN}"),
        (1022, b"[true]"),
        (1009, b"[1]"),
        (2951, b"{not-a-uuid}"),
        (2951, b"[\"not-a-uuid\"]"),
        (1001, b"{bad-bytes}"),
        (1001, b"[\"\\\\xABC\"]"),
        (1182, b"{2026-02-30}"),
        (1182, b"[\"2026-02-30\"]"),
        (1183, b"{24:00:00}"),
        (1183, b"[\"24:00:00\"]"),
        (1115, b"{not-a-timestamp}"),
        (1115, b"[\"not-a-timestamp\"]"),
        (1014, b"[true]"),
        (1015, b"[{}]"),
        (1007, b"[[1]]"),
    ];
    let mut cycles = Vec::new();
    for (oid, payload) in cases {
        for result_format in [0, 1] {
            // Describe before Bind can legitimately emit a type-only
            // descriptor. Omit it for the decoder-before-metadata oracle.
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((oid, 0, Some(payload))),
                result_format,
                false,
            ));
            cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-text-declared-domain", &[], cycles);

    // Assert
    for pair in batches.as_chunks::<2>().0 {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("08P01"));
        assert_eq!(
            pair[0]
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
                .count(),
            0
        );
        assert_eq!(
            pair[0].last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("1".to_string())]]);
    }
}

#[test]
fn should_preserve_integer_array_text_endpoints_in_both_spellings() {
    // Arrange
    let cases: [IntegerArrayEndpoint; 3] = [
        (
            1005,
            b"{-32768,32767,NULL}",
            b"[-32768,32767,null]",
            b"[-32768,32767,null]",
            b"\0\0\0\x01\0\0\0\x01\0\0\0\x15\0\0\0\x03\0\0\0\x01\0\0\0\x02\x80\0\0\0\0\x02\x7f\xff\xff\xff\xff\xff",
        ),
        (
            1007,
            b"{-2147483648,2147483647,NULL}",
            b"[-2147483648,2147483647,null]",
            b"[-2147483648,2147483647,null]",
            b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x03\0\0\0\x01\0\0\0\x04\x80\0\0\0\0\0\0\x04\x7f\xff\xff\xff\xff\xff\xff\xff",
        ),
        (
            1016,
            b"{-9223372036854775808,9223372036854775807,9007199254740993,NULL}",
            b"[-9223372036854775808,9223372036854775807,9007199254740993,null]",
            b"[-9223372036854775808,9223372036854775807,9007199254740993,null]",
            b"\0\0\0\x01\0\0\0\x01\0\0\0\x14\0\0\0\x04\0\0\0\x01\0\0\0\x08\x80\0\0\0\0\0\0\0\0\0\0\x08\x7f\xff\xff\xff\xff\xff\xff\xff\0\0\0\x08\0\x20\0\0\0\0\0\x01\xff\xff\xff\xff",
        ),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (oid, brace, bracket, text, binary) in cases {
        for input in [brace, bracket] {
            for format in [0, 1] {
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((oid, 0, Some(input))),
                    format,
                    true,
                ));
                expectations.push((oid, format, if format == 0 { text } else { binary }));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-text-integer-endpoints", &[], cycles);

    // Assert
    for ((oid, format, expected), frames) in expectations.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (oid, -1, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(expected)]
        );
    }
}

#[test]
fn should_round_trip_multidimensional_integer_array_shape_over_pgwire() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT $1::INT4[] AS value",
        Some((1007, 0, Some(&b"{{1,NULL},{3,4}}"[..]))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-multidimensional-input", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("multidimensional array RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(1007, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{{1,NULL},{3,4}}".to_owned())]]
    );
}

#[test]
fn should_preserve_array_dimensions_in_dimension_functions() {
    // Arrange
    let input = b"[0:2]={7,NULL,9}";
    let sql = "SELECT array_dims($1::INT[]) AS dimensions, array_lower($1::INT[], 1) AS lower_bound, array_upper($1::INT[], 1) AS upper_bound, array_length($1::INT[], 1) AS length, cardinality($1::INT[]) AS cardinality, array_position($1::INT[], NULL) AS null_position";
    let cycle = fixture::execute_cycle(sql, Some((1007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-bounds-and-functions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array dimensions RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(25, -1), (23, 4), (23, 4), (23, 4), (23, 4), (23, 4)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[0:2]".to_owned()),
            Some("0".to_owned()),
            Some("2".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("1".to_owned()),
        ]]
    );
}

#[test]
fn should_report_array_dimensions_null_search_positions() {
    // Arrange
    let input = b"{7,NULL,9}";
    let sql = "SELECT array_dims($1::INT[]) AS dimensions, array_lower($1::INT[], 1) AS lower_bound, array_upper($1::INT[], 1) AS upper_bound, array_length($1::INT[], 1) AS length, cardinality($1::INT[]) AS cardinality, array_position($1::INT[], NULL) AS null_position";
    let cycle = fixture::execute_cycle(sql, Some((1007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-dimensions-and-functions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array dimensions RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(25, -1), (23, 4), (23, 4), (23, 4), (23, 4), (23, 4)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[1:3]".to_owned()),
            Some("1".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("2".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_array_functions_to_nondefault_bounds() {
    // Arrange
    let input = b"[0:2]={7,NULL,9}";
    let sql = "SELECT array_append($1::INT[], 11)::TEXT, array_remove($1::INT[], NULL)::TEXT, array_replace($1::INT[], 7, 8)::TEXT, array_positions($1::INT[], NULL)::TEXT";
    let cycle = fixture::execute_cycle(sql, Some((1007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-null-functions", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("[0:3]={7,NULL,9,11}".to_owned()),
            Some("[0:1]={7,9}".to_owned()),
            Some("[0:2]={8,NULL,9}".to_owned()),
            Some("{1}".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_array_functions_to_null_elements() {
    // Arrange
    let input = b"{7,NULL,9}";
    let sql = "SELECT array_append($1::INT[], 11)::TEXT, array_remove($1::INT[], NULL)::TEXT, array_replace($1::INT[], 7, 8)::TEXT, array_positions($1::INT[], NULL)::TEXT";
    let cycle = fixture::execute_cycle(sql, Some((1007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-null-elements", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("{7,NULL,9,11}".to_owned()),
            Some("{7,9}".to_owned()),
            Some("{8,NULL,9}".to_owned()),
            Some("{2}".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_nondefault_array_parameters_across_index_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_bound_keys (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "CREATE INDEX issue_807_array_bound_keys_items_idx ON issue_807_array_bound_keys USING btree (items)",
            Vec::new(),
        ),
    ];
    let nondefault_bounds = b"[0:2]={7,NULL,9}";
    let insert = fixture::execute_cycle(
        "INSERT INTO issue_807_array_bound_keys VALUES (1, $1)",
        Some((1007, 0, Some(nondefault_bounds))),
        0,
        false,
    );
    let query = fixture::execute_cycle(
        "SELECT items::TEXT FROM issue_807_array_bound_keys WHERE items = $1 ORDER BY id",
        Some((1007, 0, Some(nondefault_bounds))),
        0,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "type-array-bounds-index-restart",
        &setup,
        vec![vec![insert], vec![query]],
    );

    // Assert
    fixture::assert_success(&stages[0][0]);
    assert!(stages[0][0].iter().any(|(tag, _)| *tag == b'C'));
    let frames = &stages[1][0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:2]={7,NULL,9}".to_owned())]],
        "the nondefault lower bound survives the btree index and restart"
    );
}

#[test]
fn should_insert_standard_oid_array_parameter_through_pgwire() {
    // Arrange
    let setup = [(
        "CREATE TABLE issue_807_array_standard_keys (id INT, items INT[])",
        Vec::new(),
    )];
    let input = b"{7,NULL,9}";
    let insert = fixture::execute_cycle(
        "INSERT INTO issue_807_array_standard_keys VALUES (1, $1)",
        Some((1007, 0, Some(input))),
        0,
        false,
    );

    // Act
    let batches = fixture::run_cycles("type-array-standard-oid-dml", &setup, vec![insert]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        frames
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"INSERT 0 1\0"[..])
    );
}

#[test]
fn should_round_trip_nondefault_array_bounds_across_index_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_binary_input (id INT, items INT[])",
            Vec::new(),
        ),
        (
            "CREATE INDEX issue_807_array_binary_input_items_idx ON issue_807_array_binary_input USING btree (items)",
            Vec::new(),
        ),
    ];
    let input = b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x03\0\0\0\0\0\0\0\x04\0\0\0\x07\xff\xff\xff\xff\0\0\0\x04\0\0\0\x09";
    let insert = fixture::execute_cycle(
        "INSERT INTO issue_807_array_binary_input VALUES (1, $1)",
        Some((1007, 1, Some(input))),
        0,
        false,
    );
    let lookup = fixture::execute_cycle(
        "SELECT items FROM issue_807_array_binary_input WHERE items = $1",
        Some((1007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let stages = fixture::run_cycles_across_restart(
        "type-array-binary-lower-bound",
        &setup,
        vec![vec![insert], vec![lookup]],
    );

    // Assert
    let frames = &stages[0][0];
    fixture::assert_success(frames);
    assert_eq!(
        frames
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"INSERT 0 1\0"[..])
    );
    let frames = &stages[1][0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:2]={7,NULL,9}".to_owned())]]
    );
}

#[test]
fn should_send_integer_arrays_with_postgresql_oid_binary_format() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_807_array_binary_output (items INT[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_binary_output VALUES ($1)",
            vec![Value::Json(serde_json::json!([7, null, 9]))],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT items FROM issue_807_array_binary_output",
        None,
        1,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-binary-output", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(
            b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x03\0\0\0\x01\0\0\0\x04\0\0\0\x07\xff\xff\xff\xff\0\0\0\x04\0\0\0\x09"
        )]
    );
}

#[test]
fn should_round_trip_multidimensional_text_array_bounds() {
    // Arrange
    let input = b"[0:1][4:5]={{1,2},{3,4}}";
    let cycle = fixture::execute_cycle(
        "SELECT $1::INT4[] AS value",
        Some((1007, 0, Some(input))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-text-multidimensional-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:1][4:5]={{1,2},{3,4}}".to_owned())]]
    );
}

#[test]
fn should_fill_multidimensional_arrays_from_dimension_array() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_fill(7, $1::INT[])::TEXT",
        Some((1007, 0, Some(&b"{2,3}"[..]))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-fill-dimensions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{{7,7,7},{7,7,7}}".to_owned())]]
    );
}

#[test]
fn should_join_array_values_using_explicit_null_replacement() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_to_string($1::INT[], ',', 'NULL')",
        Some((1007, 0, Some(&b"{1,NULL,3}"[..]))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-to-string-null-replacement", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("1,NULL,3".to_owned())]]
    );
}

#[test]
fn should_parse_delimited_text_array_null_tokens() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT string_to_array('a,NULL,c', ',', 'NULL')",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-string-to-array-null-token", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1009, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{a,NULL,c}".to_owned())]]
    );
}

#[test]
fn should_aggregate_integer_rows_into_a_postgresql_int4_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_array_aggregate_values (value INTEGER)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_array_aggregate_values VALUES (2)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_AGG(value) FROM issue_810_array_aggregate_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-array-aggregate-values", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("{2}".to_owned())]]);
}

#[test]
fn should_order_values_in_postgresql_int4_array_aggregate() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_array_aggregate_input (value INTEGER)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_array_aggregate_input VALUES (2), (1), (3)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_AGG(value ORDER BY value) FROM issue_810_array_aggregate_input",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-ordered-array-aggregate", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{1,2,3}".to_owned())]]
    );
}

#[test]
fn should_canonicalize_string_backed_array_text_elements_before_direct_output() {
    // Arrange
    let cases: [(i32, &[u8], &[u8]); 11] = [
        (1182, b"{2026-2-3,NULL}", b"[\"2026-02-03\",null]"),
        (1182, b"[\"2026-2-3\",null]", b"[\"2026-02-03\",null]"),
        (1183, b"{1:2:3.1,NULL}", b"[\"01:02:03.100000\",null]"),
        (1183, b"[\"1:2:3.1\",null]", b"[\"01:02:03.100000\",null]"),
        (
            1115,
            b"{\"2026-10-05T01:00:00+01:00\",NULL}",
            b"[\"2026-10-05T00:00:00.000000Z\",null]",
        ),
        (
            1115,
            b"[\"2026-10-05T01:00:00+01:00\",null]",
            b"[\"2026-10-05T00:00:00.000000Z\",null]",
        ),
        (
            2951,
            b"{550E8400-E29B-41D4-A716-446655440000,NULL}",
            b"[\"550e8400-e29b-41d4-a716-446655440000\",null]",
        ),
        (
            2951,
            b"[\"550E8400-E29B-41D4-A716-446655440000\",null]",
            b"[\"550e8400-e29b-41d4-a716-446655440000\",null]",
        ),
        (1001, b"{\"\\\\xABCD\",NULL}", b"[\"\\\\xabcd\",null]"),
        (1001, b"[\"\\\\xABCD\",null]", b"[\"\\\\xabcd\",null]"),
        (1000, b"{yes,NULL}", b"[true,null]"),
    ];
    let cycles = cases
        .iter()
        .map(|(oid, payload, _)| {
            fixture::execute_cycle("SELECT $1 AS v", Some((*oid, 0, Some(*payload))), 0, true)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-array-text-direct-canonical", &[], cycles);

    // Assert
    for ((oid, _, expected), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (oid, -1, -1), 0, true);
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(expected)]
        );
    }
}

#[test]
fn should_preserve_unlimited_varchar_values_across_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE finite_unlimited_varchar (k INT, v VARCHAR)",
            Vec::new(),
        ),
        (
            "CREATE TABLE finite_unlimited_varchar_array (k INT, v VARCHAR[])",
            Vec::new(),
        ),
    ];
    let scalar_inputs = [Some(&b"unlimited"[..]), Some(&b""[..]), None];
    let array_inputs = [Some(&b"[\"unlimited\"]"[..]), Some(&b"[]"[..]), None];
    let array_nonempty_binary =
        &b"\0\0\0\x01\0\0\0\0\0\0\x04\x13\0\0\0\x01\0\0\0\x01\0\0\0\x09unlimited"[..];
    let array_empty_binary = &b"\0\0\0\0\0\0\0\0\0\0\x04\x13"[..];
    let mut writes = Vec::new();
    let mut write_expectations = Vec::new();
    for format in [0, 1] {
        for (index, (scalar, array)) in scalar_inputs.into_iter().zip(array_inputs).enumerate() {
            let key = usize::try_from(format).expect("result format") * 3 + index + 1;
            writes.push(fixture::execute_cycle(
                &format!(
                    "INSERT INTO finite_unlimited_varchar (k, v) VALUES ({key}, $1) RETURNING v"
                ),
                Some((1043, 0, scalar)),
                format,
                true,
            ));
            write_expectations.push((1043, format, scalar));
            writes.push(fixture::execute_cycle(
                &format!("INSERT INTO finite_unlimited_varchar_array (k, v) VALUES ({key}, $1) RETURNING v"),
                Some((1015, 0, array)),
                format,
                true,
            ));
            let expected = if format == 1 {
                match index {
                    0 => Some(array_nonempty_binary),
                    1 => Some(array_empty_binary),
                    _ => None,
                }
            } else {
                array
            };
            write_expectations.push((1015, format, expected));
        }
    }
    let reads = unlimited_varchar_reads();
    let write_count = writes.len();
    writes.extend(reads.clone());

    // Act
    let stages = fixture::run_cycles_across_restart(
        "type-array-unlimited-varchar",
        &setup,
        vec![writes, reads],
    );

    // Assert
    for ((oid, format, expected), frames) in write_expectations.into_iter().zip(&stages[0]) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, -1, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![expected.map_or_else(|| vec![0, 1, 255, 255, 255, 255], fixture::one_field_row,)]
        );
    }
    for frames in [&stages[0][write_count..], stages[1].as_slice()] {
        for (index, frame) in frames.iter().enumerate() {
            fixture::assert_success(frame);
            let format = i16::try_from(index / 2).expect("result format");
            let scalar = index % 2 == 0;
            fixture::assert_single_column_descriptors(
                frame,
                (if scalar { 1043 } else { 1015 }, -1, -1),
                format,
                true,
            );
            let values: [Option<&[u8]>; 3] = if scalar {
                scalar_inputs
            } else if format == 1 {
                [Some(array_nonempty_binary), Some(array_empty_binary), None]
            } else {
                array_inputs
            };
            let expected = values.map(|value| {
                value.map_or_else(|| vec![0, 1, 255, 255, 255, 255], fixture::one_field_row)
            });
            assert_eq!(fixture::data_row_payloads(frame), expected.to_vec());
        }
    }
}

fn unlimited_varchar_reads() -> Vec<fixture::Cycle> {
    [0, 1]
        .into_iter()
        .flat_map(|format| {
            [
                fixture::execute_cycle(
                    "SELECT v FROM finite_unlimited_varchar WHERE k <= 3 ORDER BY k",
                    None,
                    format,
                    true,
                ),
                fixture::execute_cycle(
                    "SELECT v FROM finite_unlimited_varchar_array WHERE k <= 3 ORDER BY k",
                    None,
                    format,
                    true,
                ),
            ]
        })
        .collect()
}

#[test]
fn should_preserve_declared_float_array_text_rounding() {
    // Arrange
    let inputs = [
        b"{9007199254740993,NULL}".as_slice(),
        b"[9007199254740993,null]".as_slice(),
    ];
    let text = b"[9007199254740992.0,null]";
    let binary = b"\0\0\0\x01\0\0\0\x01\0\0\x02\xbd\0\0\0\x02\0\0\0\x01\0\0\0\x08\x43\x40\0\0\0\0\0\0\xff\xff\xff\xff";
    let cycles = inputs
        .into_iter()
        .flat_map(|input| {
            [0, 1].map(move |format| {
                fixture::execute_cycle("SELECT $1 AS v", Some((1022, 0, Some(input))), format, true)
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-array-float-input-rounding", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for (index, frames) in batches.iter().enumerate() {
        let format = i16::try_from(index % 2).expect("format");
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (1022, -1, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(if format == 0 {
                text
            } else {
                binary
            })]
        );
    }
}

#[test]
fn should_preserve_scalar_float_bits_in_both_array_spellings() {
    // Arrange
    let cases = [
        (
            b"51.248178375505404".as_slice(),
            b"\x40\x49\x9f\xc4\x4f\x1b\x2f\x60".as_slice(),
        ),
        (
            b"-93.31137037688033".as_slice(),
            b"\xc0\x57\x53\xed\x7e\x04\x69\x3b".as_slice(),
        ),
        (
            b"-36.573994842753436".as_slice(),
            b"\xc0\x42\x49\x78\xa9\xba\xd9\x6e".as_slice(),
        ),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (token, bits) in cases {
        let text = std::str::from_utf8(token).expect("decimal token");
        let inputs = [format!("{{{text},NULL}}"), format!("[{text},null]")];
        let mut binary = b"\0\0\0\x01\0\0\0\x01\0\0\x02\xbd\0\0\0\x02\0\0\0\x01\0\0\0\x08".to_vec();
        binary.extend_from_slice(bits);
        binary.extend_from_slice(b"\xff\xff\xff\xff");
        for format in [0, 1] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((701, 0, Some(token))),
                format,
                true,
            ));
            expectations.push((
                701,
                8,
                format,
                if format == 0 {
                    token.to_vec()
                } else {
                    bits.to_vec()
                },
            ));
            for input in &inputs {
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((1022, 0, Some(input.as_bytes()))),
                    format,
                    true,
                ));
                expectations.push((
                    1022,
                    -1,
                    format,
                    if format == 0 {
                        format!("[{text},null]").into_bytes()
                    } else {
                        binary.clone()
                    },
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-float-decimal-bits", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 18);
    for ((oid, length, format, payload), frames) in expectations.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, length, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&payload)]
        );
    }
}

#[test]
fn should_preserve_finite_float_array_tokens_before_output() {
    // Arrange
    let cases = [
        (
            "1.7976931348623157e308",
            b"\x7f\xef\xff\xff\xff\xff\xff\xff".as_slice(),
        ),
        (
            "-1.7976931348623157e308",
            b"\xff\xef\xff\xff\xff\xff\xff\xff".as_slice(),
        ),
        ("2.2250738585072014e-308", b"\0\x10\0\0\0\0\0\0".as_slice()),
        ("5e-324", b"\0\0\0\0\0\0\0\x01".as_slice()),
        ("-0.0", b"\x80\0\0\0\0\0\0\0".as_slice()),
        ("1e-400", b"\0\0\0\0\0\0\0\0".as_slice()),
        (
            "1.7976931348623158e308",
            b"\x7f\xef\xff\xff\xff\xff\xff\xff".as_slice(),
        ),
        ("2.4703282292062328e-324", b"\0\0\0\0\0\0\0\x01".as_slice()),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (token, bits) in cases {
        cycles.push(fixture::execute_cycle(
            "SELECT $1 AS v",
            Some((701, 0, Some(token.as_bytes()))),
            1,
            true,
        ));
        expectations.push((701, 8, bits.to_vec()));
        for input in [format!("{{{token},NULL}}"), format!("[{token},null]")] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((1022, 0, Some(input.as_bytes()))),
                1,
                true,
            ));
            let mut binary =
                b"\0\0\0\x01\0\0\0\x01\0\0\x02\xbd\0\0\0\x02\0\0\0\x01\0\0\0\x08".to_vec();
            binary.extend_from_slice(bits);
            binary.extend_from_slice(b"\xff\xff\xff\xff");
            expectations.push((1022, -1, binary));
        }
    }
    let invalid = [
        "[1e400]",
        r#"["51.248178375505404"]"#,
        "[true]",
        "[{}]",
        "[[]]",
        "[51.248178375505404,]",
        "[51.248178375505404] trailing",
        "[51.248178375505404,null",
    ];
    for input in invalid {
        for format in [0, 1] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS v",
                Some((1022, 0, Some(input.as_bytes()))),
                format,
                false,
            ));
            cycles.push(fixture::execute_cycle("SELECT 1 AS v", None, 0, true));
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-float-finite-tokens", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 56);
    for ((oid, length, payload), frames) in expectations.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, length, -1), 1, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&payload)]
        );
    }
    for pair in batches[24..].as_chunks::<2>().0 {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("08P01"));
        assert!(
            !pair[0]
                .iter()
                .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(
            fixture::data_row_payloads(&pair[1]),
            vec![fixture::one_field_row(b"1")]
        );
    }
}

#[test]
fn should_preserve_integer_array_negative_zero_in_both_spellings() {
    // Arrange
    let cases = [
        (1005, 21, b"\0\0".as_slice()),
        (1007, 23, b"\0\0\0\0".as_slice()),
        (1016, 20, b"\0\0\0\0\0\0\0\0".as_slice()),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (oid, element_oid, zero) in cases {
        let mut binary = b"\0\0\0\x01\0\0\0\x01".to_vec();
        binary.extend_from_slice(&i32::to_be_bytes(element_oid));
        binary.extend_from_slice(b"\0\0\0\x02\0\0\0\x01");
        binary.extend_from_slice(
            &i32::try_from(zero.len())
                .expect("integer width")
                .to_be_bytes(),
        );
        binary.extend_from_slice(zero);
        binary.extend_from_slice(b"\xff\xff\xff\xff");
        for input in [b"{-0,NULL}".as_slice(), b"[-0,null]".as_slice()] {
            for format in [0, 1] {
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((oid, 0, Some(input))),
                    format,
                    true,
                ));
                expectations.push((
                    oid,
                    format,
                    if format == 0 {
                        b"[0,null]".to_vec()
                    } else {
                        binary.clone()
                    },
                ));
            }
        }
    }
    for (oid, _, _) in cases {
        for input in [
            b"[-0.0]".as_slice(),
            b"[0.0]".as_slice(),
            b"[0e0]".as_slice(),
        ] {
            for format in [0, 1] {
                cycles.push(fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((oid, 0, Some(input))),
                    format,
                    false,
                ));
                cycles.push(fixture::execute_cycle("SELECT 1 AS v", None, 0, true));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-array-integer-negative-zero", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 48);
    for ((oid, format, payload), frames) in expectations.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, -1, -1), format, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&payload)]
        );
    }
    for pair in batches[12..].as_chunks::<2>().0 {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("08P01"));
        assert!(
            !pair[0]
                .iter()
                .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(
            fixture::data_row_payloads(&pair[1]),
            vec![fixture::one_field_row(b"1")]
        );
    }
}

#[test]
fn should_preserve_character_array_input_without_wire_modifiers() {
    // Arrange
    let inputs = [r#"{"éA  ",NULL}"#.as_bytes(), r#"["éA  ",null]"#.as_bytes()];
    let expected_text = r#"["éA  ",null]"#.as_bytes();
    let cases = [
        (1014, b"\0\0\0\x01\0\0\0\x01\0\0\x04\x12\0\0\0\x02\0\0\0\x01\0\0\0\x05\xc3\xa9A  \xff\xff\xff\xff".as_slice()),
        (1015, b"\0\0\0\x01\0\0\0\x01\0\0\x04\x13\0\0\0\x02\0\0\0\x01\0\0\0\x05\xc3\xa9A  \xff\xff\xff\xff".as_slice()),
    ];
    let cycles = cases
        .into_iter()
        .flat_map(|(oid, _)| {
            inputs.into_iter().flat_map(move |input| {
                [0, 1].map(move |format| {
                    fixture::execute_cycle(
                        "SELECT $1 AS v",
                        Some((oid, 0, Some(input))),
                        format,
                        true,
                    )
                })
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-array-character-unknown-modifier", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 8);
    for ((oid, binary), group) in cases.into_iter().zip(batches.as_chunks::<4>().0) {
        for (index, frames) in group.iter().enumerate() {
            let format = i16::try_from(index % 2).expect("format");
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (oid, -1, -1), format, true);
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(if format == 0 {
                    expected_text
                } else {
                    binary
                })]
            );
        }
    }
}

#[test]
fn should_subscript_an_array_at_its_second_element() {
    // Arrange
    let sql = "SELECT (ARRAY[2,4,6])[2]";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-subscript", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array subscript RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![Some("4".to_owned())]]);
}

#[test]
fn should_apply_any_all_empty_array_truth_tables() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT 4 = ANY ($1), 4 = ALL ($1)",
        Some((1007, 0, Some(b"{}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-any-all-empty", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("quantified comparison RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("f".to_owned()), Some("t".to_owned())]]
    );
}

#[test]
fn should_compare_any_all_against_a_bound_integer_array() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT 4 = ANY ($1), 4 = ALL ($1)",
        Some((1007, 0, Some(b"{2,4,6}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("type-array-any-all-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("quantified comparison RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned()), Some("f".to_owned())]]
    );
}
