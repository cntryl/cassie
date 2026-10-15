//! Timezone-aware `JSONPath` first-result functions through pgwire.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn assert_jsonb_result(frames: &fixture::Frames, expected: Option<&str>) {
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(3802, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![expected.map(str::to_owned)]]
    );
}

#[test]
fn should_return_jsonb_identity_given_a_timezone_sensitive_path_when_query_first_tz_matches() {
    // Arrange
    let sql = "SELECT jsonb_path_query_first_tz(\
        '{\"at\":\"2024-11-03T01:30:00-04:00\"}'::JSONB, \
        '$.at ? (@.datetime() > \"2024-11-03T00:00:00-04:00\".datetime())'::JSONPATH\
    )";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("issue-810-jsonpath-first-tz-match", &[], cycles);

    // Assert
    assert_jsonb_result(&batches[0], Some("\"2024-11-03T01:30:00-04:00\""));
}

#[test]
fn should_return_null_given_a_timezone_sensitive_path_when_query_first_tz_has_no_match() {
    // Arrange
    let sql = "SELECT jsonb_path_query_first_tz(\
        '{\"at\":\"2024-11-03T01:30:00-04:00\"}'::JSONB, \
        '$.at ? (@.datetime() > \"2024-11-03T03:00:00-04:00\".datetime())'::JSONPATH\
    )";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("issue-810-jsonpath-first-tz-no-match", &[], cycles);

    // Assert
    assert_jsonb_result(&batches[0], None);
}
