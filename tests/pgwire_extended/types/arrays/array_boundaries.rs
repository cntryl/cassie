//! PostgreSQL 18 array subquery construction tracked by #807.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;

fn array_subquery_setup() -> [(&'static str, Vec<Value>); 2] {
    [
        (
            "CREATE TABLE issue_807_array_subquery_input (ordinal INT4 PRIMARY KEY, item INT4)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_807_array_subquery_input VALUES (2, NULL), (1, 10), (3, 30)",
            Vec::new(),
        ),
    ]
}

fn assert_array_descriptor(frames: &fixture::Frames, format: i16) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    for (index, columns) in descriptions.iter().enumerate() {
        let expected_format = if index == 0 { 0 } else { format };
        assert_eq!(columns.len(), 1);
        let column = &columns[0];
        assert_eq!(
            (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            ),
            (1_007, -1, -1, expected_format)
        );
    }
}

fn binary_data_row(field: &[u8]) -> Vec<u8> {
    let mut row = 1_i16.to_be_bytes().to_vec();
    row.extend_from_slice(
        &i32::try_from(field.len())
            .expect("array binary field length")
            .to_be_bytes(),
    );
    row.extend_from_slice(field);
    row
}

#[test]
fn should_construct_array_from_ordered_subquery_with_null_elements() {
    // Arrange
    let sql =
        "SELECT ARRAY(SELECT item FROM issue_807_array_subquery_input ORDER BY ordinal) AS value";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles(
        "issue-807-array-ordered-subquery",
        &array_subquery_setup(),
        cycles,
    );

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_array_descriptor(&frames, format);
        if format == 0 {
            assert_eq!(
                wire::data_rows(&frames),
                vec![vec![Some("{10,NULL,30}".to_owned())]]
            );
        } else {
            let populated = [
                0, 0, 0, 1, // dimensions
                0, 0, 0, 1, // has NULL elements
                0, 0, 0, 23, // INT4 element OID
                0, 0, 0, 3, // element count
                0, 0, 0, 1, // lower bound
                0, 0, 0, 4, 0, 0, 0, 10, // first element
                255, 255, 255, 255, // SQL NULL element
                0, 0, 0, 4, 0, 0, 0, 30, // last element
            ];
            assert_eq!(
                fixture::data_row_payloads(&frames),
                vec![binary_data_row(&populated)]
            );
        }
    }
}

#[test]
fn should_return_typed_empty_array_for_zero_row_subquery() {
    // Arrange
    let sql =
        "SELECT ARRAY(SELECT item FROM issue_807_array_subquery_input WHERE ordinal = 99) AS value";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles(
        "issue-807-array-empty-subquery",
        &array_subquery_setup(),
        cycles,
    );

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_array_descriptor(&frames, format);
        if format == 0 {
            assert_eq!(wire::data_rows(&frames), vec![vec![Some("{}".to_owned())]]);
        } else {
            let empty = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 23];
            assert_eq!(
                fixture::data_row_payloads(&frames),
                vec![binary_data_row(&empty)]
            );
        }
    }
}

#[test]
fn should_preserve_array_lower_bounds_when_prepending_scalar_elements() {
    // Arrange
    let sql = "SELECT array_lower(array_prepend(0, '[2:3]={1,2}'::int[]),1), array_upper(array_prepend(0, '[2:3]={1,2}'::int[]),1), array_lower(array_prepend(0, '[-2:-1]={1,2}'::int[]),1), array_upper(array_prepend(0, '[-2:-1]={1,2}'::int[]),1)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-prepend-lower-bound", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("2".to_owned()),
            Some("4".to_owned()),
            Some("-2".to_owned()),
            Some("0".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_native_identity_for_array_function_outputs() {
    // Arrange
    let sql = "SELECT array_prepend(0, '[2:3]={1,2}'::int[]) AS prepended, array_fill(7, ARRAY[2]) AS filled, array_prepend(0, '[2:3]={1,2}'::int[])::text AS formatted";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-array-function-output", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    for (_, payload) in batches[0].iter().filter(|(tag, _)| *tag == b'T') {
        let fields = wire::parse_row_description(payload);
        assert_eq!(
            fields
                .iter()
                .map(|field| field.type_oid)
                .collect::<Vec<_>>(),
            vec![1_007, 1_007, 25]
        );
    }
    assert_eq!(
        wire::data_rows(&batches[0])[0][2],
        Some("[2:4]={0,1,2}".to_owned())
    );
}

#[test]
fn should_concatenate_scalar_elements_with_arrays_in_both_operand_orders() {
    // Arrange
    let sql = "SELECT (0 || '[2:3]={1,2}'::int[])::text, ('[2:3]={1,2}'::int[] || 3)::text";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-array-scalar-concat", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("[2:4]={0,1,2}".to_owned()),
            Some("[2:4]={1,2,3}".to_owned()),
        ]]
    );
}

#[test]
fn should_distinguish_typed_text_elements_from_unknown_array_text_during_concatenation() {
    // Arrange
    let sql = "SELECT ('x'::text || ARRAY['a'])::text, (ARRAY['a'] || 'x'::text)::text, ('{2}' || ARRAY[1])::text, (ARRAY[1] || '{2}')::text";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-array-text-overloads", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("{x,a}".to_owned()),
            Some("{a,x}".to_owned()),
            Some("{2,1}".to_owned()),
            Some("{1,2}".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_array_concatenation_parameter_overloads() {
    // Arrange
    let cases = [
        (
            [23, 1_007],
            [Some("0"), Some("[2:3]={1,2}")],
            [23, 1_007],
            "[2:4]={0,1,2}",
        ),
        (
            [1_007, 23],
            [Some("[2:3]={1,2}"), Some("3")],
            [1_007, 23],
            "[2:4]={1,2,3}",
        ),
        ([25, 1_009], [Some("x"), Some("{a}")], [25, 1_009], "{x,a}"),
        (
            [23, 1_007],
            [None, Some("[2:3]={1,2}")],
            [23, 1_007],
            "[2:4]={NULL,1,2}",
        ),
        (
            [1_007, 1_007],
            [None, Some("[2:3]={1,2}")],
            [1_007, 1_007],
            "[2:3]={1,2}",
        ),
        (
            [0, 1_007],
            [Some("{0}"), Some("[2:3]={1,2}")],
            [1_007, 1_007],
            "{0,1,2}",
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(oids, values, _, _)| {
            let values = values.map(|value| value.map(str::as_bytes));
            vec![
                wire::parse_frame_with_types("", "SELECT ($1 || $2)::text", oids),
                wire::describe_statement_frame(""),
                wire::bind_frame_with_formats("", "", &[0, 0], &values, &[]),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-807-array-concat-parameters", &[], cycles);

    // Assert
    for (frames, (_, _, expected_oids, expected)) in batches.iter().zip(cases) {
        fixture::assert_success(frames);
        let (_, parameters) = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter types");
        assert_eq!(wire::parse_parameter_description(parameters), expected_oids);
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![Some(expected.to_owned())]]
        );
    }
}
