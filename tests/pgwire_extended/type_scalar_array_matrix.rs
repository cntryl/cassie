//! All14 supported private scalar-array families, excluding VECTOR[] identity.

use super::support_pgwire as wire;
use super::support_type_codec_matrix as matrix;
use super::support_type_metadata_contract as fixture;
use cassie::types::Value;

#[test]
fn should_preserve_scalar_array_codec_identity_across_wire_formats() {
    // Arrange
    let setup = matrix::setup_queries("array_values", &matrix::ARRAY_CASES);
    let (cycles, expected) = matrix::populated_cycles("array_values", &matrix::ARRAY_CASES);

    // Act
    let batches = fixture::run_cycles("type-array-codecs", &matrix::setup_refs(&setup), cycles);

    // Assert
    assert_eq!(matrix::ARRAY_CASES.len(), 14);
    assert_eq!(batches.len(), 84);
    matrix::assert_batches(&batches, &expected);
}

#[test]
fn should_preserve_scalar_array_descriptors_for_empty_outputs() {
    // Arrange
    let setup = matrix::setup_queries("array_empty", &matrix::ARRAY_CASES);
    let (cycles, expected) = matrix::descriptor_cycles("array_empty", &matrix::ARRAY_CASES, false);

    // Act
    let batches = fixture::run_cycles(
        "type-array-empty-results",
        &matrix::setup_refs(&setup),
        cycles,
    );

    // Assert
    assert_eq!(batches.len(), 28);
    matrix::assert_batches(&batches, &expected);
}

#[test]
fn should_preserve_scalar_array_descriptors_for_null_outputs() {
    // Arrange
    let (cycles, expected) = matrix::descriptor_cycles("unused", &matrix::ARRAY_CASES, true);

    // Act
    let batches = fixture::run_cycles("type-array-null-results", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 28);
    matrix::assert_batches(&batches, &expected);
}

#[test]
fn should_preserve_scalar_array_empty_value_framing() {
    // Arrange
    let setup = matrix::setup_queries("array_empty_values", &matrix::ARRAY_CASES);
    let (cycles, expected) = matrix::empty_array_cycles("array_empty_values");

    // Act
    let batches = fixture::run_cycles(
        "type-array-empty-values",
        &matrix::setup_refs(&setup),
        cycles,
    );

    // Assert
    assert_eq!(batches.len(), 84);
    matrix::assert_batches(&batches, &expected);
}

#[test]
fn should_preserve_array_window_result_carriers() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE output_array_windows (n INT, arr INT[])",
            vec![],
        ),
        (
            "INSERT INTO output_array_windows (n, arr) VALUES (1, $1)",
            vec![Value::Json(serde_json::json!([]))],
        ),
        (
            "INSERT INTO output_array_windows (n, arr) VALUES (2, $1)",
            vec![Value::Json(serde_json::json!([2, null]))],
        ),
        (
            "INSERT INTO output_array_windows (n, arr) VALUES (3, NULL)",
            vec![],
        ),
    ];
    let cases = ["FIRST_VALUE", "LAST_VALUE"]
        .into_iter()
        .flat_map(|function| {
            [false, true].into_iter().flat_map(move |parameterized| {
                [false, true].into_iter().flat_map(move |describe| {
                    [0, 1].map(move |format| (function, parameterized, describe, format))
                })
            })
        })
        .collect::<Vec<_>>();
    let cycles = cases.iter().map(|(function, parameterized, describe, format)| {
        let predicate = if *parameterized { " WHERE $1" } else { "" };
        let sql = format!("SELECT {function}(arr) OVER (PARTITION BY n ORDER BY n) AS arr FROM output_array_windows{predicate} ORDER BY n");
        fixture::execute_cycle(&sql, parameterized.then_some((16, 0, Some(&b"true"[..]))), *format, *describe)
    }).collect();
    let empty_binary = &b"\0\0\0\0\0\0\0\0\0\0\0\x17"[..];
    let populated_binary =
        &b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x02\0\0\0\x01\0\0\0\x04\0\0\0\x02\xff\xff\xff\xff"
            [..];

    // Act
    let batches = fixture::run_cycles("type-array-window-carriers", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 16);
    for ((function, parameterized, describe, format), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (1007, -1, -1), format, describe);
        let (empty, populated) = if format == 1 {
            (empty_binary, populated_binary)
        } else {
            (&b"[]"[..], &b"[2,null]"[..])
        };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![
                fixture::one_field_row(empty),
                fixture::one_field_row(populated),
                vec![0, 1, 255, 255, 255, 255],
            ],
            "{function}, parameterized={parameterized}, described={describe}, format={format}"
        );
    }
}

#[test]
fn should_sort_array_elements_with_nulls_after_non_null_values() {
    // Arrange
    let input = b"{3,NULL,1,2}";
    let sql = "SELECT array_sort($1::INT[])::TEXT AS sorted";
    let cycle = fixture::execute_cycle(sql, Some((1_007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-array-sort-null-order", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"{1,2,3,NULL}")]
    );
}

#[test]
fn should_reverse_array_elements_without_collapsing_null_slots() {
    // Arrange
    let input = b"{1,NULL,3}";
    let sql = "SELECT array_reverse($1::INT[])::TEXT";
    let cycle = fixture::execute_cycle(sql, Some((1_007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-array-reverse-null-slot", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{3,NULL,1}".to_owned())]]
    );
}

#[test]
fn should_trim_only_the_requested_array_tail_elements() {
    // Arrange
    let input = b"{1,2,3,4}";
    let sql = "SELECT trim_array($1::INT[], 2)::TEXT";
    let cycle = fixture::execute_cycle(sql, Some((1_007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-trim-array-tail", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{1,2}".to_owned())]]
    );
}

#[test]
fn should_sample_the_requested_number_of_array_elements() {
    // Arrange
    let input = b"{1,2,3,4}";
    let sql = "SELECT length(array_sample($1::INT[], 2)::TEXT) = 5";
    let cycle = fixture::execute_cycle(sql, Some((1_007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-array-sample-size", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_shuffle_arrays_without_changing_their_element_count() {
    // Arrange
    let input = b"{1,2,3,4}";
    let sql = "SELECT length(array_shuffle($1::INT[])::TEXT) = 9";
    let cycle = fixture::execute_cycle(sql, Some((1_007, 0, Some(input))), 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-array-shuffle-cardinality", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}
