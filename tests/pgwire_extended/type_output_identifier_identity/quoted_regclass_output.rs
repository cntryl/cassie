//! Quoted `regclass` identity formatting over PostgreSQL wire.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_quoted_regclass_output_across_wire_formats() {
    // Arrange
    let setup = [
        ("CREATE SCHEMA \"Issue_818_Quoted\"", Vec::new()),
        (
            "CREATE TABLE \"Issue_818_Quoted\".\"Relation.Part\" (id INTEGER)",
            Vec::new(),
        ),
        ("SET search_path TO pg_catalog, public", Vec::new()),
    ];
    let sql = "SELECT '\"Issue_818_Quoted\".\"Relation.Part\"'::regclass \
                      AS object_identity, \
                      '\"Issue_818_Quoted\".\"Relation.Part\"'::regclass::oid \
                      AS object_oid, \
                      '\"Issue_818_Quoted\".\"Relation.Part\"'::regclass::text \
                      AS canonical_name";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles("regclass-quoted-output-wire", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 2);
    for (format, frames) in [0, 1].into_iter().zip(&batches) {
        assert_eq!(wire::error_code(frames), None);
        fixture::assert_success(frames);
        let descriptions = frames
            .iter()
            .filter(|(tag, _)| *tag == b'T')
            .map(|(_, body)| wire::parse_row_description(body))
            .collect::<Vec<_>>();
        assert_eq!(descriptions.len(), 2);
        for (index, columns) in descriptions.iter().enumerate() {
            assert_eq!(columns.len(), 3);
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.name.as_str())
                    .collect::<Vec<_>>(),
                ["object_identity", "object_oid", "canonical_name"]
            );
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.type_oid)
                    .collect::<Vec<_>>(),
                [2_205, 26, 25]
            );
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.type_size)
                    .collect::<Vec<_>>(),
                [4, 4, -1]
            );
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.type_mod)
                    .collect::<Vec<_>>(),
                [-1, -1, -1]
            );
            let expected_format = if index == 0 { 0 } else { format };
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.format_code)
                    .collect::<Vec<_>>(),
                [expected_format; 3]
            );
        }

        let rows = fixture::data_row_payloads(frames);
        assert_eq!(rows.len(), 1);
        if format == 0 {
            let fields = wire::parse_data_row(&rows[0]);
            assert_eq!(fields.len(), 3);
            assert_eq!(
                fields[0].as_deref(),
                Some("\"Issue_818_Quoted\".\"Relation.Part\"")
            );
            assert!(
                fields[1]
                    .as_ref()
                    .is_some_and(|oid| oid.parse::<u32>().is_ok_and(|value| value > 0))
            );
            assert_eq!(
                fields[2].as_deref(),
                Some("\"Issue_818_Quoted\".\"Relation.Part\"")
            );
        } else {
            let fields = super::raw_data_row_fields(&rows[0]);
            assert_eq!(fields.len(), 3);
            assert_eq!(fields[0].len(), 4);
            assert_eq!(fields[0], fields[1]);
            assert_eq!(fields[2], b"\"Issue_818_Quoted\".\"Relation.Part\"");
        }
    }
}
