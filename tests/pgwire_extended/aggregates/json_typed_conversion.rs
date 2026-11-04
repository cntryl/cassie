//! Exact JSON aggregate values and native document identities over pgwire.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

const VALUE: &str = "9007199254740993.125500";
const PARAMETER_SQL: &str = "SELECT json_agg($1::NUMERIC), jsonb_agg($1::NUMERIC), \
    json_object_agg('n',$1::NUMERIC), jsonb_object_agg('n',$1::NUMERIC)";

fn assert_document_values(frames: &[(u8, Vec<u8>)], result_format: i16) {
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .rev()
        .find(|(tag, _)| *tag == b'T')
        .expect("document result descriptor");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [114, 3802, 114, 3802]
    );
    assert!(columns
        .iter()
        .all(|column| column.format_code == result_format));
    let version = if result_format == 1 { "\u{1}" } else { "" };
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(format!("[{VALUE}]")),
            Some(format!("{version}[{VALUE}]")),
            Some(format!("{{ \"n\" : {VALUE} }}")),
            Some(format!("{version}{{\"n\": {VALUE}}}")),
        ]]
    );
}

#[test]
fn should_preserve_exact_json_aggregate_numbers_in_simple_query_results() {
    // Arrange
    let sql = PARAMETER_SQL.replace("$1", VALUE);
    let cycle = vec![wire::simple_query_frame(&sql)];

    // Act
    let batches = fixture::run_cycles("issue-811-json-aggregate-simple", &[], vec![cycle]);

    // Assert
    assert_document_values(&batches[0], 0);
}

#[test]
fn should_preserve_json_aggregate_oids_across_numeric_wire_formats() {
    // Arrange
    let numeric = cassie::types::extended::numeric::parse_text(VALUE)
        .expect("finite numeric fixture")
        .send_binary();
    let mut cycles = Vec::new();
    for parameter_format in [0, 1] {
        for result_format in [0, 1] {
            let parameter = if parameter_format == 0 {
                VALUE.as_bytes()
            } else {
                &numeric
            };
            let statement = format!("json_collection_{parameter_format}_{result_format}");
            cycles.push(vec![
                wire::parse_frame(&statement, PARAMETER_SQL),
                wire::describe_statement_frame(&statement),
                wire::bind_frame_with_formats(
                    "",
                    &statement,
                    &[parameter_format],
                    &[Some(parameter)],
                    &[result_format],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]);
        }
    }

    // Act
    let batches = fixture::run_cycles("issue-811-json-aggregate-formats", &[], cycles);

    // Assert
    for (index, frames) in batches.iter().enumerate() {
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("numeric parameter descriptor");
        assert_eq!(wire::parse_parameter_description(&parameters.1), [1700]);
        assert_document_values(frames, if index % 2 == 0 { 0 } else { 1 });
    }
}

#[test]
fn should_reject_duplicate_formatted_json_keys_without_publishing_partial_results() {
    // Arrange
    let candidate = "SELECT jsonb_object_agg_unique_strict(k,v) \
        FROM (VALUES ('NaN'::FLOAT8,NULL::INT),('NaN'::FLOAT8,1)) AS inputs(k,v)";
    let cycles = vec![
        vec![wire::simple_query_frame(candidate)],
        vec![wire::simple_query_frame("SELECT 43::INT AS recovered")],
    ];

    // Act
    let batches = fixture::run_cycles("issue-811-json-key-conversion-recovery", &[], cycles);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22030"));
    assert!(batches[0]
        .iter()
        .all(|(tag, _)| !matches!(*tag, b'D' | b'C')));
    assert_eq!(
        batches[0].last().map(|(_, body)| body.as_slice()),
        Some(&b"I"[..])
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("43".into())]]);
}
