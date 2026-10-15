//! PostgreSQL 18 set-returning function error contracts tracked by #808.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[path = "parameterized_unnest.rs"]
mod parameterized_unnest;
#[path = "rows_from_empty_member/nulls.rs"]
mod rows_from_empty_member;
#[path = "../types/temporal/timestamp_without_timezone_series/semantics.rs"]
mod timestamp_without_timezone_series;
#[path = "../types/temporal/timestamptz_timezone_series/parameters.rs"]
mod timestamptz_timezone_series;

#[test]
fn should_reject_zero_steps_for_every_generate_series_type_with_22023() {
    // Arrange
    let queries = [
        "SELECT generate_series(1, 3, 0)",
        "SELECT generate_series(1::BIGINT, 3::BIGINT, 0::BIGINT)",
        "SELECT generate_series(1::NUMERIC, 3::NUMERIC, 0::NUMERIC)",
        "SELECT generate_series(TIMESTAMP '2024-01-01', TIMESTAMP '2024-01-03', INTERVAL '0 days')",
        "SELECT generate_series(TIMESTAMPTZ '2024-01-01 00:00+00', TIMESTAMPTZ '2024-01-03 00:00+00', INTERVAL '0 days')",
    ];
    let mut cycles = queries
        .into_iter()
        .map(|sql| fixture::execute_cycle(sql, None, 0, true))
        .collect::<Vec<_>>();
    cycles.push(fixture::execute_cycle("SELECT 7::INT4", None, 0, true));

    // Act
    let batches = fixture::run_cycles("issue-808-generate-series-zero-step", &[], cycles);

    // Assert
    for (sql, frames) in queries.iter().zip(&batches) {
        assert_eq!(
            wire::error_code(frames).as_deref(),
            Some("22023"),
            "zero-step SQLSTATE for {sql}"
        );
        assert_eq!(
            wire::data_rows(frames),
            Vec::<Vec<Option<String>>>::new(),
            "partial rows for {sql}"
        );
        assert_eq!(
            frames.iter().filter(|(tag, _)| *tag == b'C').count(),
            0,
            "CommandComplete for failing query {sql}"
        );
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..]),
            "connection did not return to idle after {sql}"
        );
    }
    fixture::assert_success(&batches[queries.len()]);
    assert_eq!(
        wire::data_rows(&batches[queries.len()]),
        vec![vec![Some("7".to_owned())]]
    );
}
