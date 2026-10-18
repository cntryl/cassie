//! PostgreSQL 18.6 scalar-cast observations that distinguish older rejection contracts.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use crate::support_wire_bind as bind;

fn assert_text_row(frames: &fixture::Frames, oids: &[i32], values: &[&str]) {
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        oids
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![values
            .iter()
            .map(|value| Some((*value).to_string()))
            .collect::<Vec<_>>()]
    );
}

#[test]
fn should_encode_special_float_values_given_valid_spellings_when_selected_as_text_and_binary() {
    // Arrange
    let sql = "SELECT CAST('inf' AS FLOAT), CAST('-infinity' AS FLOAT), CAST('nan' AS FLOAT)";
    let cycles = vec![
        vec![wire::simple_query_frame(sql)],
        bind::execute_cycle(sql, None, 1, true),
    ];

    // Act
    let batches = fixture::run_cycles("float-special-cast-codecs", &[], cycles);

    // Assert
    assert_text_row(
        &batches[0],
        &[701, 701, 701],
        &["Infinity", "-Infinity", "NaN"],
    );
    fixture::assert_success(&batches[1]);
    let payloads = bind::data_row_payloads(&batches[1]);
    let mut expected = 3_i16.to_be_bytes().to_vec();
    for bits in [
        0x7ff0_0000_0000_0000_u64,
        0xfff0_0000_0000_0000,
        0x7ff8_0000_0000_0000,
    ] {
        expected.extend_from_slice(&8_i32.to_be_bytes());
        expected.extend_from_slice(&bits.to_be_bytes());
    }
    assert_eq!(payloads, vec![expected]);
    for (_, body) in batches[1].iter().filter(|(tag, _)| *tag == b'T') {
        assert_eq!(
            wire::parse_row_description(body)
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![701, 701, 701]
        );
    }
}

#[test]
fn should_return_numeric_out_of_range_given_finite_float8_overflow_when_cast() {
    // Arrange
    let sql = "SELECT CAST('1e400' AS FLOAT)";

    // Act
    let batches = fixture::run_cycles(
        "float-cast-overflow-state",
        &[],
        vec![
            vec![wire::simple_query_frame("SELECT CAST('3.5' AS FLOAT)")],
            vec![wire::simple_query_frame(sql)],
            vec![wire::simple_query_frame("SELECT CAST('3.5' AS FLOAT)")],
        ],
    );

    // Assert
    assert_text_row(&batches[0], &[701], &["3.5"]);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22003"));
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_text_row(&batches[2], &[701], &["3.5"]);
}

#[test]
fn should_truncate_character_values_given_explicit_length_casts_when_selected() {
    // Arrange
    let sql = "SELECT CAST('abcd' AS char(3)), CAST('abcd' AS varchar(3))";

    // Act
    let batches = fixture::run_cycles(
        "character-explicit-truncation",
        &[],
        vec![vec![wire::simple_query_frame(sql)]],
    );

    // Assert
    assert_text_row(&batches[0], &[1042, 1043], &["abc", "abc"]);
}

#[test]
fn should_reject_overlength_assignment_given_character_columns_when_inserted_without_casts() {
    // Arrange
    let setup = [
        ("CREATE TABLE char_assignment (v char(3))", vec![]),
        ("CREATE TABLE varchar_assignment (v varchar(3))", vec![]),
    ];

    // Act
    let batches = fixture::run_cycles(
        "character-assignment-state",
        &setup,
        vec![
            vec![wire::simple_query_frame(
                "INSERT INTO char_assignment VALUES ('abcd')",
            )],
            vec![wire::simple_query_frame(
                "INSERT INTO varchar_assignment VALUES ('abcd')",
            )],
            vec![wire::simple_query_frame(
                "SELECT count(*) FROM char_assignment",
            )],
            vec![wire::simple_query_frame(
                "SELECT count(*) FROM varchar_assignment",
            )],
        ],
    );

    // Assert
    for frames in &batches[..2] {
        assert_eq!(wire::error_code(frames).as_deref(), Some("22001"));
        assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
    }
    assert_text_row(&batches[2], &[20], &["0"]);
    assert_text_row(&batches[3], &[20], &["0"]);
}

#[test]
fn should_preserve_timestamp_zone_identity_given_offset_input_when_timezone_is_utc() {
    // Arrange
    let local = "SELECT CAST('2024-01-01T09:00:00+02:00' AS TIMESTAMP)";
    let zoned = "SELECT CAST('2024-01-01T09:00:00+02:00' AS TIMESTAMP WITH TIME ZONE)";

    // Act
    let batches = fixture::run_cycles(
        "timestamp-zone-cast-identity",
        &[],
        vec![
            vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")],
            vec![wire::simple_query_frame(local)],
            vec![wire::simple_query_frame(zoned)],
        ],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_text_row(&batches[1], &[1114], &["2024-01-01 09:00:00"]);
    assert_text_row(&batches[2], &[1184], &["2024-01-01 07:00:00+00"]);
}
