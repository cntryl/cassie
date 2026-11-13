//! pgwire framing of enums: `RowDescription` OIDs, text and binary results,
//! parameters typed by the enum OID and error reporting.
//! Authored against the v0.4.0 contract.

use crate::support_enum_types::{run_with_enum_oids, EnumOids};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

const SETUP: [&str; 3] = [
    "CREATE TYPE state_code AS ENUM ('open', 'closed')",
    "CREATE TABLE jobs (id int PRIMARY KEY, st state_code, history state_code[])",
    "INSERT INTO jobs VALUES (1, 'open', '{open,closed}'), (2, 'closed', NULL), (3, NULL, NULL)",
];

fn descriptions(frames: &fixture::Frames) -> Vec<Vec<wire::RowDescription>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect()
}

fn parameter_descriptions(frames: &fixture::Frames) -> Vec<Vec<i32>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b't')
        .map(|(_, body)| wire::parse_parameter_description(body))
        .collect()
}

fn describe_only(sql: &str) -> fixture::Cycle {
    vec![
        wire::parse_frame_with_types("", sql, &[]),
        wire::describe_statement_frame(""),
        wire::sync_frame(),
    ]
}

#[test]
fn should_describe_an_enum_column_with_the_enum_oid() {
    // Arrange
    let build = |_| vec![fixture::execute_cycle("SELECT st FROM jobs", None, 0, true)];

    // Act
    let (oids, batches) = run_with_enum_oids("enum-wire-oid", &SETUP, build);

    // Assert
    fixture::assert_success(&batches[0]);
    let described = descriptions(&batches[0]);
    assert_eq!(described.len(), 2, "statement and portal descriptions");
    for columns in described {
        assert_eq!(columns[0].type_oid, oids.type_oid);
        assert!(oids.type_oid > 16_383, "an allocated, non-builtin OID");
        assert_eq!(columns[0].type_size, 4, "typlen 4");
    }
}

#[test]
fn should_describe_an_enum_array_column_with_the_array_oid() {
    // Arrange
    let build = |_| {
        vec![fixture::execute_cycle(
            "SELECT history FROM jobs",
            None,
            0,
            true,
        )]
    };

    // Act
    let (oids, batches) = run_with_enum_oids("enum-wire-array-oid", &SETUP, build);

    // Assert
    fixture::assert_success(&batches[0]);
    for columns in descriptions(&batches[0]) {
        assert_eq!(columns[0].type_oid, oids.array_oid);
        assert_ne!(oids.array_oid, oids.type_oid);
        assert_eq!(columns[0].type_size, -1);
    }
}

#[test]
fn should_describe_a_cast_to_an_enum_with_the_enum_oid() {
    // Arrange
    let build = |_| {
        vec![fixture::execute_cycle(
            "SELECT 'open'::state_code",
            None,
            0,
            true,
        )]
    };

    // Act
    let (oids, batches) = run_with_enum_oids("enum-wire-cast-oid", &SETUP, build);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(descriptions(&batches[0])[0][0].type_oid, oids.type_oid);
}

#[test]
fn should_send_enum_labels_as_text() {
    // Arrange
    let build = |_| {
        vec![fixture::execute_cycle(
            "SELECT st, history FROM jobs ORDER BY id",
            None,
            0,
            true,
        )]
    };

    // Act
    let (_, batches) = run_with_enum_oids("enum-wire-text", &SETUP, build);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![
            vec![Some("open".to_owned()), Some("{open,closed}".to_owned())],
            vec![Some("closed".to_owned()), None],
            vec![None, None],
        ]
    );
}

#[test]
fn should_send_enum_labels_as_utf8_bytes_in_binary_results() {
    // Arrange
    let build = |_| {
        vec![fixture::execute_cycle(
            "SELECT st FROM jobs ORDER BY id",
            None,
            1,
            false,
        )]
    };

    // Act
    let (_, batches) = run_with_enum_oids("enum-wire-binary", &SETUP, build);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![
            fixture::one_field_row(b"open"),
            fixture::one_field_row(b"closed"),
            vec![0, 1, 255, 255, 255, 255],
        ]
    );
}

#[test]
fn should_send_a_multibyte_label_as_utf8_in_both_formats() {
    // Arrange
    let setup = [
        "CREATE TYPE state_code AS ENUM ('caf\u{e9}', 'na\u{ef}ve')",
        "CREATE TABLE jobs (id int PRIMARY KEY, st state_code)",
        "INSERT INTO jobs VALUES (1, 'caf\u{e9}')",
    ];
    let build = |_| {
        vec![
            fixture::execute_cycle("SELECT st FROM jobs", None, 0, false),
            fixture::execute_cycle("SELECT st FROM jobs", None, 1, false),
        ]
    };

    // Act
    let (_, batches) = run_with_enum_oids("enum-wire-utf8", &setup, build);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("caf\u{e9}".to_owned())]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row("caf\u{e9}".as_bytes())]
    );
}

#[test]
fn should_accept_a_text_parameter_typed_by_the_enum_oid() {
    // Arrange
    let build = |oids: EnumOids| {
        vec![fixture::execute_cycle(
            "SELECT id FROM jobs WHERE st = $1",
            Some((oids.type_oid, 0, Some(b"closed".as_slice()))),
            0,
            false,
        )]
    };

    // Act
    let (_, batches) = run_with_enum_oids("enum-wire-param-text", &SETUP, build);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("2".to_owned())]]
    );
}

#[test]
fn should_accept_a_binary_parameter_typed_by_the_enum_oid() {
    // Arrange
    let build = |oids: EnumOids| {
        vec![fixture::execute_cycle(
            "SELECT id FROM jobs WHERE st = $1",
            Some((oids.type_oid, 1, Some(b"open".as_slice()))),
            0,
            false,
        )]
    };

    // Act
    let (_, batches) = run_with_enum_oids("enum-wire-param-binary", &SETUP, build);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("1".to_owned())]]
    );
}

#[test]
fn should_accept_an_array_parameter_typed_by_the_array_oid() {
    // Arrange
    let build = |oids: EnumOids| {
        vec![fixture::execute_cycle(
            "SELECT id FROM jobs WHERE st = ANY($1) ORDER BY id",
            Some((oids.array_oid, 0, Some(b"{open,closed}".as_slice()))),
            0,
            false,
        )]
    };

    // Act
    let (_, batches) = run_with_enum_oids("enum-wire-param-array", &SETUP, build);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
}

#[test]
fn should_accept_an_untyped_parameter_cast_to_the_enum() {
    // Arrange
    let build = |_| {
        vec![fixture::execute_cycle(
            "SELECT ($1::state_code)::text",
            Some((0, 0, Some(b"closed".as_slice()))),
            0,
            false,
        )]
    };

    // Act
    let (_, batches) = run_with_enum_oids("enum-wire-param-untyped", &SETUP, build);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("closed".to_owned())]]
    );
}

#[test]
fn should_describe_a_cast_parameter_with_the_enum_oid() {
    // Arrange
    let build = |_| {
        vec![
            describe_only("SELECT $1::state_code"),
            describe_only("SELECT id FROM jobs WHERE st = $1"),
        ]
    };

    // Act
    let (oids, batches) = run_with_enum_oids("enum-wire-param-description", &SETUP, build);

    // Assert
    assert_eq!(
        parameter_descriptions(&batches[0]),
        vec![vec![oids.type_oid]]
    );
    assert_eq!(
        parameter_descriptions(&batches[1]),
        vec![vec![oids.type_oid]]
    );
}

#[test]
fn should_reject_an_invalid_label_parameter_with_sqlstate_22p02() {
    // Arrange
    let build = |oids: EnumOids| {
        vec![
            fixture::execute_cycle(
                "SELECT id FROM jobs WHERE st = $1",
                Some((oids.type_oid, 0, Some(b"bogus".as_slice()))),
                0,
                false,
            ),
            fixture::execute_cycle(
                "SELECT id FROM jobs WHERE st = $1",
                Some((oids.type_oid, 1, Some(b"bogus".as_slice()))),
                0,
                false,
            ),
            fixture::execute_cycle(
                "SELECT ($1::state_code)::text",
                Some((0, 0, Some(b"bogus".as_slice()))),
                0,
                false,
            ),
        ]
    };

    // Act
    let (_, batches) = run_with_enum_oids("enum-wire-param-invalid", &SETUP, build);

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch).as_deref(), Some("22P02"));
    }
}

#[test]
fn should_report_the_dependent_column_in_the_drop_restrict_detail() {
    // Arrange
    let build = |_| {
        vec![fixture::execute_cycle(
            "DROP TYPE state_code",
            None,
            0,
            false,
        )]
    };

    // Act
    let (_, batches) = run_with_enum_oids("enum-wire-drop-detail", &SETUP, build);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("2BP01"));
    let error = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, body)| wire::parse_error_fields(body))
        .expect("an ErrorResponse");
    let detail = error
        .iter()
        .find(|(kind, _)| *kind == 'D')
        .map(|(_, text)| text.as_str())
        .expect("a DETAIL field");
    assert!(
        detail.contains("column st of table jobs"),
        "detail names the dependent column"
    );
}
