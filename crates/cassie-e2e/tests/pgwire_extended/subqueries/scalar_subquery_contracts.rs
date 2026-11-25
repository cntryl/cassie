//! Residual PostgreSQL 18 scalar and row-subquery contracts for #762.

use std::sync::Arc;
use std::time::Duration;

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture_with_config;
use cassie::config::CassieRuntimeConfig;

fn assert_subquery_result(
    frames: &[(u8, Vec<u8>)],
    expected_oids: &[i32],
    expected_rows: &[Vec<Option<String>>],
    context: &str,
) {
    let errors = frames
        .iter()
        .filter(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(
        wire::error_code(frames),
        None,
        "{context} ErrorResponse: {errors:?}"
    );
    let description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .unwrap_or_else(|| panic!("{context} must return RowDescription"));
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids,
        "{context} result type OIDs"
    );
    assert_eq!(
        wire::data_rows(frames).as_slice(),
        expected_rows,
        "{context} values"
    );
}

include!("semantics.rs");

include!("../sequences/binary.rs");

include!("../operators/semantics.rs");

include!("nulls.rs");

include!("binary.rs");

include!("resources.rs");
