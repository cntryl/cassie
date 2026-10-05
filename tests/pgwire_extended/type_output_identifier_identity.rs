use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

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
