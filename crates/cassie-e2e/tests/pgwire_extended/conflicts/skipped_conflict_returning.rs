//! Finite CON-10 qualification over ordinary simple and declared Bind ingress.

use super::support_pgwire as wire;
use super::support_skipped_conflict_returning_wire::{run, Execution, Frames, Transcript};

fn expected_columns(key: &str) -> Vec<wire::RowDescription> {
    [(key, 20, 8), ("note", 25, -1)]
        .into_iter()
        .map(|(name, type_oid, type_size)| wire::RowDescription {
            name: name.into(),
            table_oid: 0,
            attr_num: 0,
            type_oid,
            type_size,
            type_mod: -1,
            format_code: 0,
        })
        .collect()
}

fn assert_result(frames: &Frames, tags: &[u8], key: &str, command: &[u8], rows: &[(&str, &str)]) {
    assert_eq!(wire::error_code(frames), None, "{frames:?}");
    assert_eq!(frames.iter().map(|frame| frame.0).collect::<Vec<_>>(), tags);
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    let descriptions = frames
        .iter()
        .filter(|frame| frame.0 == b'T')
        .map(|frame| wire::parse_row_description(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(descriptions, vec![expected_columns(key)]);
    let commands = frames
        .iter()
        .filter(|frame| frame.0 == b'C')
        .map(|frame| frame.1.as_slice())
        .collect::<Vec<_>>();
    assert_eq!(commands, vec![command]);
    let actual = wire::data_rows(frames);
    assert!(actual.iter().all(|row| row.len() == 2));
    let expected = rows
        .iter()
        .map(|(id, note)| vec![Some((*id).into()), Some((*note).into())])
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

fn assert_execution(execution: &Execution, bound: bool, retry: bool) {
    // A completed-portal shortcut bypasses query execution and these counters.
    assert_eq!(execution.after.count, execution.before.count + 1);
    assert_eq!(
        execution.after.rows,
        execution.before.rows + if retry { 0 } else { 2 }
    );
    assert_eq!(execution.after.errors, execution.before.errors);
    let tags = match (bound, retry) {
        (false, false) => &b"TDDCZ"[..],
        (false, true) => &b"TCZ"[..],
        (true, false) => &b"2TDDCZ"[..],
        (true, true) => &b"2TCZ"[..],
    };
    let rows = if retry {
        &[][..]
    } else {
        &[("3", "third"), ("1", "first")][..]
    };
    let command = if retry {
        &b"INSERT 0 0\0"[..]
    } else {
        &b"INSERT 0 2\0"[..]
    };
    assert_result(&execution.frames, tags, "row_key", command, rows);
    assert_result(
        &execution.state,
        b"TDDDCZ",
        "id",
        b"SELECT 3\0",
        &[("1", "first"), ("2", "old"), ("3", "third")],
    );
}

fn assert_transcript(transcript: &Transcript, bound: bool) {
    assert_result(
        &transcript.seed,
        b"TDCZ",
        "id",
        b"SELECT 1\0",
        &[("2", "old")],
    );
    assert_eq!(transcript.parsed.is_some(), bound);
    if let Some(parsed) = &transcript.parsed {
        assert_eq!(wire::error_code(parsed), None);
        assert_eq!(
            parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            b"1Z"
        );
        assert_eq!(
            parsed.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    assert_execution(&transcript.first, bound, false);
    assert_execution(&transcript.retry, bound, true);
    assert_result(
        &transcript.retired_state,
        b"TDDDCZ",
        "id",
        b"SELECT 3\0",
        &[("1", "first"), ("2", "old"), ("3", "third")],
    );
}

#[test]
fn should_match_skipped_conflict_returning_over_simple_query() {
    // Arrange
    let label = "skipped-returning-simple";

    // Act
    let transcript = run(label, None);

    // Assert
    assert_transcript(&transcript, false);
}

#[test]
fn should_match_skipped_conflict_returning_over_declared_text_bind() {
    // Arrange
    let label = "skipped-returning-text";

    // Act
    let transcript = run(label, Some(false));

    // Assert
    assert_transcript(&transcript, true);
}

#[test]
fn should_match_skipped_conflict_returning_over_declared_binary_bind() {
    // Arrange
    let label = "skipped-returning-binary";

    // Act
    let transcript = run(label, Some(true));

    // Assert
    assert_transcript(&transcript, true);
}

const ENTITY_TABLE: &str =
    "CREATE TABLE entities (id BIGINT PRIMARY KEY, rev BIGINT, payload TEXT)";
const APPLY_REVISION: &str = "ON CONFLICT (id) DO UPDATE SET rev = EXCLUDED.rev, \
    payload = EXCLUDED.payload WHERE target.rev < EXCLUDED.rev RETURNING id, rev, payload";
const ENTITY_READBACK: &str = "SELECT id, rev, payload FROM entities";

fn assert_entity_result(frames: &Frames, command: &str, expected: Option<[&str; 3]>) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .collect::<Vec<_>>();
    let [columns] = descriptions.as_slice() else {
        panic!("expected one row description for {command}: {frames:?}");
    };
    assert_eq!(
        columns
            .iter()
            .map(|c| (c.name.as_str(), c.type_oid))
            .collect::<Vec<_>>(),
        [("id", 20), ("rev", 20), ("payload", 25)]
    );
    let commands = frames
        .iter()
        .filter(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| payload.as_slice())
        .collect::<Vec<_>>();
    assert_eq!(commands, [format!("{command}\0").as_bytes()]);
    let expected = expected
        .into_iter()
        .map(|row| row.map(|value| Some(value.to_owned())).to_vec())
        .collect::<Vec<_>>();
    assert_eq!(wire::data_rows(frames), expected, "{command}");
}

fn assert_replayed_events(responses: &[Frames]) {
    let [initial, newer, duplicate, stale, readback] = responses else {
        panic!("expected four writes and one readback");
    };
    assert_entity_result(initial, "INSERT 0 1", Some(["1", "1", "initial"]));
    assert_entity_result(newer, "INSERT 0 1", Some(["1", "3", "new"]));
    assert_entity_result(duplicate, "INSERT 0 0", None);
    assert_entity_result(stale, "INSERT 0 0", None);
    assert_entity_result(readback, "SELECT 1", Some(["1", "3", "new"]));
}

#[test]
fn should_apply_only_newer_entity_revisions_over_simple_query() {
    // Arrange
    let mut script = Vec::new();
    for (rev, payload) in [(1, "initial"), (3, "new"), (3, "duplicate"), (2, "stale")] {
        script.push(vec![wire::simple_query_frame(&format!(
            "INSERT INTO entities AS target VALUES (1, {rev}, '{payload}') {APPLY_REVISION}"
        ))]);
    }
    script.push(vec![wire::simple_query_frame(ENTITY_READBACK)]);

    // Act
    let responses = crate::support_repeatable_read::run_own_script(&[ENTITY_TABLE], script);

    // Assert
    assert_replayed_events(&responses);
}

fn bound_entity_command(statement: &str, parameters: &[&str]) -> Vec<Vec<u8>> {
    vec![
        wire::bind_frame("", statement, parameters),
        wire::describe_portal_frame(""),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]
}

#[test]
fn should_report_parameterized_entity_mutations() {
    // Arrange
    let parse = vec![
        wire::parse_frame_with_types(
            "upsert",
            &format!("INSERT INTO entities AS target VALUES ($1, $2, $3) {APPLY_REVISION}"),
            &[20, 20, 25],
        ),
        wire::parse_frame_with_types(
            "revise",
            "UPDATE entities SET payload = $2 WHERE id = $1 RETURNING id, rev, payload",
            &[20, 25],
        ),
        wire::parse_frame_with_types(
            "remove",
            "DELETE FROM entities WHERE id = $1 RETURNING id, rev, payload",
            &[20],
        ),
        wire::sync_frame(),
    ];
    let mut script = vec![parse];
    for (rev, payload) in [
        ("1", "initial"),
        ("3", "new"),
        ("3", "duplicate"),
        ("2", "stale"),
    ] {
        script.push(bound_entity_command("upsert", &["1", rev, payload]));
    }
    script.extend([
        vec![wire::simple_query_frame(ENTITY_READBACK)],
        bound_entity_command("revise", &["2", "missing"]),
        bound_entity_command("remove", &["2"]),
        bound_entity_command("revise", &["1", "revised"]),
        bound_entity_command("remove", &["1"]),
        vec![wire::simple_query_frame(ENTITY_READBACK)],
    ]);

    // Act
    let responses = crate::support_repeatable_read::run_own_script(&[ENTITY_TABLE], script);

    // Assert
    assert_replayed_events(&responses[1..6]);
    assert_entity_result(&responses[6], "UPDATE 0", None);
    assert_entity_result(&responses[7], "DELETE 0", None);
    assert_entity_result(&responses[8], "UPDATE 1", Some(["1", "3", "revised"]));
    assert_entity_result(&responses[9], "DELETE 1", Some(["1", "3", "revised"]));
    assert_entity_result(&responses[10], "SELECT 0", None);
}
