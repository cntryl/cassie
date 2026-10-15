//! Binary parameter and result identity through chained PostgreSQL casts.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

type Frames = Vec<(u8, Vec<u8>)>;

fn assert_binary_cast_chain_contract(batches: &[Frames], input_value: &[u8]) {
    let control_description = &batches[1];
    let control_execution = &batches[2];
    let chained_description = &batches[4];
    let chained_execution = &batches[5];

    for (stage, frames) in [
        ("direct Parse/Describe", control_description),
        ("direct binary Bind/Execute", control_execution),
        ("cast-chain Describe", chained_description),
        ("cast-chain binary Bind/Execute", chained_execution),
    ] {
        let errors = frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>();
        assert_eq!(
            wire::error_code(frames),
            None,
            "{stage} ErrorResponse: {errors:?}"
        );
    }
    assert_eq!(
        wire::parse_parameter_description(
            &control_description
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("direct parameter description")
                .1
        ),
        [23]
    );
    assert_eq!(
        wire::parse_parameter_description(
            &chained_description
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("cast-chain parameter description")
                .1
        ),
        [23]
    );
    let control_columns = wire::parse_row_description(
        &control_description
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("direct statement RowDescription")
            .1,
    );
    let chained_columns = wire::parse_row_description(
        &chained_description
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("cast-chain statement RowDescription")
            .1,
    );
    assert_eq!(control_columns[0].type_oid, 23);
    assert_eq!(chained_columns[0].type_oid, 20);
    for (context, frames, expected_oid, expected_bytes) in [
        (
            "direct binary control",
            control_execution,
            23,
            input_value.to_vec(),
        ),
        (
            "binary cast chain",
            chained_execution,
            20,
            37_i64.to_be_bytes().to_vec(),
        ),
    ] {
        let columns = wire::parse_row_description(
            &frames
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("portal RowDescription")
                .1,
        );
        assert_eq!(
            (columns[0].type_oid, columns[0].format_code),
            (expected_oid, 1)
        );
        let data_row = frames
            .iter()
            .find(|frame| frame.0 == b'D')
            .expect("binary DataRow");
        assert_eq!(&data_row.1[6..], expected_bytes, "{context} bytes");
    }
}

#[test]
fn should_preserve_cast_chain_identity_given_binary_parameters_when_binary_results_are_requested() {
    // Arrange
    let input_value = 37_i32.to_be_bytes();
    let control_sql = "SELECT $1::INT4 AS value";
    let chained_sql = "SELECT (($1::INT4)::INT8) AS value";
    // Act
    let batches = run_script(
        "cast-chain-binary-identity",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "direct_int4",
                control_sql,
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("direct_int4"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "direct_portal",
                    "direct_int4",
                    &[1],
                    &[Some(&input_value)],
                    &[1],
                ),
                wire::describe_portal_frame("direct_portal"),
                wire::execute_frame("direct_portal"),
                wire::sync_frame(),
            ]),
            Step::ParseFlush(wire::parse_frame_with_types(
                "cast_chain",
                chained_sql,
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("cast_chain"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "cast_chain_portal",
                    "cast_chain",
                    &[1],
                    &[Some(&input_value)],
                    &[1],
                ),
                wire::describe_portal_frame("cast_chain_portal"),
                wire::execute_frame("cast_chain_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Assert
    assert_binary_cast_chain_contract(&batches, &input_value);
}

#[test]
fn should_preserve_unsized_varchar_identity_given_a_qualified_type_when_a_value_is_described() {
    // Arrange
    let sql = "SELECT 'unbounded varchar text'::pg_catalog.varchar AS value";

    // Act
    let batches = run_script(
        "issue-796-qualified-unsized-varchar",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames), None);
    let description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("qualified VARCHAR RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns[0].type_oid, 1043);
    assert_eq!(columns[0].type_mod, -1);
    assert_eq!(columns[0].format_code, 0);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("unbounded varchar text".to_owned())]]
    );
}
