//! JSONB relational keys preserve native wire identities (#809).
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_jsonb_distinct_identity_across_wire_formats() {
    // Arrange
    let sql = "SELECT DISTINCT payload FROM (VALUES ($1::JSONB),($2::JSONB)) AS docs(payload) ORDER BY payload";
    let mut cycles = Vec::new();
    for parameter_format in [0, 1] {
        for result_format in [0, 1] {
            let first = if parameter_format == 0 {
                &b"[1.0]"[..]
            } else {
                &b"\x01[1.0]"[..]
            };
            let second = if parameter_format == 0 {
                &b"[1.00]"[..]
            } else {
                &b"\x01[1.00]"[..]
            };
            let name = format!("jsonb_key_{parameter_format}_{result_format}");
            cycles.push(vec![
                wire::parse_frame(&name, sql),
                wire::describe_statement_frame(&name),
                wire::bind_frame_with_formats(
                    "",
                    &name,
                    &[parameter_format],
                    &[Some(first), Some(second)],
                    &[result_format],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]);
        }
    }
    // Act
    let batches = fixture::run_cycles("jsonb-key-wire-formats", &[], cycles);
    // Assert
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter descriptor");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            [3802, 3802]
        );
        let description = frames
            .iter()
            .rev()
            .find(|(tag, _)| *tag == b'T')
            .expect("result descriptor");
        let columns = wire::parse_row_description(&description.1);
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].type_oid, 3802);
        assert_eq!(columns[0].format_code, i16::from(index % 2 != 0));
        let rows = wire::data_rows(frames);
        assert_eq!(rows.len(), 1);
        let output = rows[0][0].as_deref().expect("nonnull document");
        let output = if index % 2 == 0 {
            output
        } else {
            output.strip_prefix('\u{1}').expect("JSONB binary version")
        };
        assert!(["[1.0]", "[1.00]"].contains(&output));
    }
}
