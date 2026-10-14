use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

#[path = "type_output_identifier_identity/regoperator_lookup_wire.rs"]
mod regoperator_lookup_wire;

#[path = "type_output_identifier_identity/quoted_regclass_output.rs"]
mod quoted_regclass_output;
#[path = "type_output_identifier_identity/regclass_search_path.rs"]
mod regclass_search_path;

#[test]
fn should_preserve_stored_identifier_identity_in_wire_output_metadata() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE output_identifier_rows (gate BOOLEAN, \"Gate\" BOOLEAN, \"a.b\" INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO output_identifier_rows (gate, \"Gate\", \"a.b\") VALUES (TRUE, FALSE, 42)",
            Vec::new(),
        ),
    ];
    let cases = identifier_cases();
    let cycles = cases
        .iter()
        .flat_map(|(sql, _, _, _, _)| {
            [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true))
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-wire-stored-identifier-identity", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((sql, name, descriptor, text, binary), pair) in
        cases.into_iter().zip(batches.as_chunks::<2>().0)
    {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            assert_eq!(wire::error_code(frames), None, "{sql}");
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, descriptor, format, true);
            for (_, description) in frames.iter().filter(|(tag, _)| *tag == b'T') {
                assert_eq!(
                    wire::parse_row_description(description)[0].name,
                    name,
                    "{sql}"
                );
            }
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(if format == 0 {
                    text
                } else {
                    binary
                })],
                "{sql}"
            );
        }
    }
}

#[test]
fn should_emit_reg_identity_wire_contract() {
    // Arrange
    let cases: [(&str, i32, &str, i32); 9] = [
        (
            "SELECT 'pg_catalog.pg_class'::regclass AS object_id",
            2_205,
            "pg_class",
            1_259,
        ),
        (
            "SELECT 'pg_catalog'::regnamespace AS object_id",
            4_089,
            "pg_catalog",
            11,
        ),
        (
            "SELECT 'pg_catalog.int4'::regtype AS object_id",
            2_206,
            "integer",
            23,
        ),
        (
            "SELECT 'pg_catalog.abs(integer)'::regprocedure AS object_id",
            2_202,
            "abs(integer)",
            1_397,
        ),
        (
            "SELECT 'pg_catalog.now'::regproc AS object_id",
            24,
            "now",
            1_299,
        ),
        (
            "SELECT 'pg_catalog.|/'::regoper AS object_id",
            2_203,
            "|/",
            596,
        ),
        (
            "SELECT 'pg_catalog.+(integer,integer)'::regoperator AS object_id",
            2_204,
            "+(integer,integer)",
            551,
        ),
        (
            "SELECT 'pg_catalog.simple'::regconfig AS object_id",
            3_734,
            "simple",
            3_748,
        ),
        (
            "SELECT 'pg_catalog.simple'::regdictionary AS object_id",
            3_769,
            "simple",
            3_765,
        ),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(sql, _, _, _)| {
            [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true))
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("reg-identity-wire-text-binary", &[], cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((sql, type_oid, text, object_oid), pair) in
        cases.into_iter().zip(batches.as_chunks::<2>().0)
    {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            assert_eq!(wire::error_code(frames), None, "{sql}");
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (type_oid, 4, -1), format, true);
            for (_, description) in frames.iter().filter(|(tag, _)| *tag == b'T') {
                assert_eq!(
                    wire::parse_row_description(description)[0].name,
                    "object_id",
                    "{sql}"
                );
            }
            let expected_field = if format == 0 {
                text.as_bytes().to_vec()
            } else {
                object_oid.to_be_bytes().to_vec()
            };
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(&expected_field)],
                "{sql}"
            );
        }
    }
}

#[test]
fn should_encode_regrole_with_its_role_oid_on_wire() {
    // Arrange
    let setup = [("CREATE ROLE issue_818_wire_role", Vec::new())];
    let sql = "SELECT 'issue_818_wire_role'::regrole AS role_identity, \
                      'issue_818_wire_role'::regrole::oid AS object_oid";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles("regrole-wire-oid-identity", &setup, cycles);

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
            assert_eq!(columns.len(), 2);
            assert_eq!([columns[0].type_oid, columns[1].type_oid], [4_096, 26]);
            assert_eq!([columns[0].type_size, columns[1].type_size], [4, 4]);
            assert_eq!([columns[0].type_mod, columns[1].type_mod], [-1, -1]);
            assert_eq!(
                [columns[0].name.as_str(), columns[1].name.as_str()],
                ["role_identity", "object_oid"]
            );
            let expected_format = if index == 0 { 0 } else { format };
            assert_eq!(
                [columns[0].format_code, columns[1].format_code],
                [expected_format, expected_format]
            );
        }
        let rows = fixture::data_row_payloads(frames);
        assert_eq!(rows.len(), 1);
        if format == 0 {
            let fields = wire::parse_data_row(&rows[0]);
            assert_eq!(fields[0].as_deref(), Some("issue_818_wire_role"));
            assert!(
                fields[1]
                    .as_ref()
                    .is_some_and(|value| value.parse::<u32>().is_ok())
            );
        } else {
            let fields = raw_data_row_fields(&rows[0]);
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].len(), 4);
            assert_eq!(fields[0], fields[1]);
        }
    }
}

#[test]
fn should_report_reg_identity_cast_sqlstates_on_wire() {
    // Arrange
    let cases = [
        ("SELECT 'issue_818_missing_relation'::regclass", "42P01"),
        ("SELECT 'issue_818_missing_type'::regtype", "42704"),
        ("SELECT 'pg_catalog.abs(text)'::regprocedure", "42883"),
        ("SELECT 'not-an-oid'::oid", "22P02"),
    ];
    let mut cycles = cases
        .iter()
        .map(|(sql, _)| fixture::execute_cycle(sql, None, 0, false))
        .collect::<Vec<_>>();
    cycles.push(fixture::execute_cycle("SELECT 42", None, 0, false));

    // Act
    let batches = fixture::run_cycles("reg-identity-wire-sqlstates", &[], cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() + 1);
    for ((sql, expected_state), frames) in cases.into_iter().zip(&batches) {
        assert_eq!(
            wire::error_code(frames).as_deref(),
            Some(expected_state),
            "{sql}"
        );
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        assert_eq!(
            frames
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
                .count(),
            0,
            "{sql} must fail before result metadata or rows"
        );
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    assert_eq!(
        wire::error_code(batches.last().expect("recovery query")),
        None
    );
    fixture::assert_success(batches.last().expect("recovery query"));
    assert_eq!(
        fixture::data_row_payloads(batches.last().expect("recovery query")),
        vec![fixture::one_field_row(b"42")]
    );
}

fn raw_data_row_fields(payload: &[u8]) -> Vec<&[u8]> {
    let count = i16::from_be_bytes(payload[..2].try_into().expect("DataRow has a field count"));
    let count = usize::try_from(count).expect("DataRow field count is nonnegative");
    let mut cursor = 2usize;
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        let end_of_length = cursor + 4;
        let length = i32::from_be_bytes(
            payload[cursor..end_of_length]
                .try_into()
                .expect("DataRow field has a length"),
        );
        let length = usize::try_from(length).expect("DataRow field is not NULL");
        cursor = end_of_length;
        let end = cursor + length;
        fields.push(&payload[cursor..end]);
        cursor = end;
    }
    fields
}

type IdentifierCase = (
    &'static str,
    &'static str,
    (i32, i16, i32),
    &'static [u8],
    &'static [u8],
);

fn identifier_cases() -> [IdentifierCase; 9] {
    [
        (
            "SELECT gate AS plain FROM output_identifier_rows",
            "plain",
            (16, 1, -1),
            &b"t"[..],
            &b"\x01"[..],
        ),
        (
            "SELECT \"Gate\" AS \"Kept\" FROM output_identifier_rows",
            "Kept",
            (16, 1, -1),
            &b"f"[..],
            &b"\x00"[..],
        ),
        (
            "SELECT output_identifier_rows.\"Gate\" AS flag FROM output_identifier_rows",
            "flag",
            (16, 1, -1),
            &b"f"[..],
            &b"\x00"[..],
        ),
        (
            "SELECT \"a.b\" AS dotted FROM output_identifier_rows",
            "dotted",
            (23, 4, -1),
            &b"42"[..],
            &b"\x00\x00\x00\x2a"[..],
        ),
        (
            "SELECT output_identifier_rows.\"a.b\" AS dotted FROM output_identifier_rows",
            "dotted",
            (23, 4, -1),
            &b"42"[..],
            &b"\x00\x00\x00\x2a"[..],
        ),
        (
            "SELECT plain FROM (SELECT gate AS plain FROM output_identifier_rows) AS d",
            "plain",
            (16, 1, -1),
            &b"t"[..],
            &b"\x01"[..],
        ),
        (
            "SELECT \"GateAlias\" FROM (SELECT gate AS \"GateAlias\" FROM output_identifier_rows) AS d",
            "GateAlias",
            (16, 1, -1),
            &b"t"[..],
            &b"\x01"[..],
        ),
        (
            "SELECT d.\"GateAlias\" AS \"KeptAlias\" FROM (SELECT gate AS \"GateAlias\" FROM output_identifier_rows) AS d",
            "KeptAlias",
            (16, 1, -1),
            &b"t"[..],
            &b"\x01"[..],
        ),
        (
            "SELECT \"a.b\" FROM (SELECT \"a.b\" FROM output_identifier_rows) AS d",
            "a.b",
            (23, 4, -1),
            &b"42"[..],
            &b"\x00\x00\x00\x2a"[..],
        ),
    ]
}
