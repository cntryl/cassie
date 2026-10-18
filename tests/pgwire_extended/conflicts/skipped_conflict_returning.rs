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
