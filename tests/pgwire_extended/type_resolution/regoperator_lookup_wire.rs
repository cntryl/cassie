//! Exact `to_regoperator` overload identities over PostgreSQL wire.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_exact_to_regoperator_overload_identity_over_pgwire() {
    // Arrange
    let sql = "SELECT to_regoperator('pg_catalog.+(integer,integer)') AS integer_add, \
                     to_regoperator('pg_catalog.+(numeric,numeric)') AS numeric_add, \
                     to_regoperator('pg_catalog.+(integer,uuid)') IS NULL AS missing_add, \
                     to_regoperator('pg_catalog.+(integer,integer)')::oid = \
                       'pg_catalog.+(integer,integer)'::regoperator::oid AS matches_cast, \
                     to_regoperator('pg_catalog.+(integer,integer)')::oid <> \
                       to_regoperator('pg_catalog.+(numeric,numeric)')::oid AS distinct_overloads";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles("to-regoperator-overload-wire", &[], cycles);

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
        for (description_index, columns) in descriptions.iter().enumerate() {
            assert_eq!(columns.len(), 5);
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.name.as_str())
                    .collect::<Vec<_>>(),
                [
                    "integer_add",
                    "numeric_add",
                    "missing_add",
                    "matches_cast",
                    "distinct_overloads"
                ]
            );
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.type_oid)
                    .collect::<Vec<_>>(),
                [2_204, 2_204, 16, 16, 16]
            );
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.type_size)
                    .collect::<Vec<_>>(),
                [4, 4, 1, 1, 1]
            );
            assert!(columns.iter().all(|column| column.type_mod == -1));
            assert_eq!(
                columns
                    .iter()
                    .map(|column| column.format_code)
                    .collect::<Vec<_>>(),
                [if description_index == 0 { 0 } else { format }; 5]
            );
        }

        let rows = fixture::data_row_payloads(frames);
        assert_eq!(rows.len(), 1);
        if format == 0 {
            assert_eq!(
                wire::parse_data_row(&rows[0]),
                vec![
                    Some("+(integer,integer)".to_owned()),
                    Some("+(numeric,numeric)".to_owned()),
                    Some("t".to_owned()),
                    Some("t".to_owned()),
                    Some("t".to_owned()),
                ]
            );
        } else {
            let fields = super::raw_data_row_fields(&rows[0]);
            assert_eq!(fields.len(), 5);
            assert_eq!(fields[0], &551_i32.to_be_bytes()[..]);
            assert_eq!(fields[1], &1_758_i32.to_be_bytes()[..]);
            assert_eq!(fields[2], &[1]);
            assert_eq!(fields[3], &[1]);
            assert_eq!(fields[4], &[1]);
        }
    }
}
