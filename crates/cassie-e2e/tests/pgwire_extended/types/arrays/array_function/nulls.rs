struct ScalarArrayCase {
    sql: &'static str,
    oid: i32,
    size: i16,
    text: Option<&'static [u8]>,
    binary: Option<&'static [u8]>,
}

struct ArrayWireExpectation {
    oid: i32,
    size: i16,
    format: i16,
    value: Option<&'static [u8]>,
}

fn array_null_cases() -> [ScalarArrayCase; 10] {
    let empty_array_binary = b"\0\0\0\0\0\0\0\0\0\0\0\x17";
    let null_element_array_binary =
        b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x01\0\0\0\x01\xff\xff\xff\xff";
    [
        ScalarArrayCase {
            sql: "SELECT NULL::INT4[]",
            oid: 1_007,
            size: -1,
            text: None,
            binary: None,
        },
        ScalarArrayCase {
            sql: "SELECT ARRAY[]::INT4[]",
            oid: 1_007,
            size: -1,
            text: Some(b"{}"),
            binary: Some(empty_array_binary),
        },
        ScalarArrayCase {
            sql: "SELECT ARRAY[NULL]::INT4[]",
            oid: 1_007,
            size: -1,
            text: Some(b"{NULL}"),
            binary: Some(null_element_array_binary),
        },
        ScalarArrayCase {
            sql: "SELECT array_dims(NULL::INT4[])",
            oid: 25,
            size: -1,
            text: None,
            binary: None,
        },
        ScalarArrayCase {
            sql: "SELECT cardinality(NULL::INT4[])",
            oid: 23,
            size: 4,
            text: None,
            binary: None,
        },
        ScalarArrayCase {
            sql: "SELECT array_dims(ARRAY[]::INT4[])",
            oid: 25,
            size: -1,
            text: None,
            binary: None,
        },
        ScalarArrayCase {
            sql: "SELECT array_length(ARRAY[]::INT4[], 1)",
            oid: 23,
            size: 4,
            text: None,
            binary: None,
        },
        ScalarArrayCase {
            sql: "SELECT cardinality(ARRAY[]::INT4[])",
            oid: 23,
            size: 4,
            text: Some(b"0"),
            binary: Some(b"\0\0\0\0"),
        },
        ScalarArrayCase {
            sql: "SELECT array_position(ARRAY[NULL]::INT4[], NULL)",
            oid: 23,
            size: 4,
            text: Some(b"1"),
            binary: Some(b"\0\0\0\x01"),
        },
        ScalarArrayCase {
            sql: "SELECT ARRAY[NULL]::INT4[] IS NULL",
            oid: 16,
            size: 1,
            text: Some(b"f"),
            binary: Some(b"\0"),
        },
    ]
}

fn array_null_expectations(cases: &[ScalarArrayCase]) -> Vec<ArrayWireExpectation> {
    let mut expectations = Vec::with_capacity(cases.len() * 2);
    for case in cases {
        for format in [0, 1] {
            expectations.push(ArrayWireExpectation {
                oid: case.oid,
                size: case.size,
                format,
                value: if format == 0 { case.text } else { case.binary },
            });
        }
    }
    expectations
}

fn array_null_cycles(cases: &[ScalarArrayCase]) -> Vec<fixture::Cycle> {
    cases
        .iter()
        .flat_map(|case| {
            [0, 1]
                .into_iter()
                .map(move |format| fixture::execute_cycle(case.sql, None, format, true))
        })
        .collect()
}

fn assert_array_null_results(
    expectations: Vec<ArrayWireExpectation>,
    batches: Vec<fixture::Frames>,
) {
    let null_field_row = vec![0, 1, 255, 255, 255, 255];
    assert_eq!(batches.len(), expectations.len());
    for (expectation, frames) in expectations.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(
            &frames,
            (expectation.oid, expectation.size, -1),
            expectation.format,
            true,
        );
        let expected_row = expectation
            .value
            .map_or_else(|| null_field_row.clone(), fixture::one_field_row);
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected_row]);
    }
}
