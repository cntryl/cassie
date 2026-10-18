//! Extended-query frame construction and raw `DataRow` observations.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract::Cycle;

pub fn execute_cycle(
    sql: &str,
    parameter: Option<(i32, i16, Option<&[u8]>)>,
    result_format: i16,
    describe: bool,
) -> Cycle {
    let oids = parameter.map_or_else(Vec::new, |(oid, _, _)| vec![oid]);
    let formats = parameter.map_or_else(Vec::new, |(_, format, _)| vec![format]);
    let params = parameter.map_or_else(Vec::new, |(_, _, body)| vec![body]);
    // Empty result formats select default text without handle_bind's explicit-
    // format Describe. No-Describe text owners must reach Execute itself.
    let result_formats = if result_format == 0 {
        Vec::new()
    } else {
        vec![result_format]
    };
    let mut frames = vec![wire::parse_frame_with_types("", sql, &oids)];
    if describe {
        frames.push(wire::describe_statement_frame(""));
    }
    frames.push(wire::bind_frame_with_formats(
        "",
        "",
        &formats,
        &params,
        &result_formats,
    ));
    if describe {
        frames.push(wire::describe_portal_frame(""));
    }
    frames.extend([wire::execute_frame(""), wire::sync_frame()]);
    frames
}

pub fn data_row_payloads(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<u8>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'D')
        .map(|(_, payload)| payload.clone())
        .collect()
}
