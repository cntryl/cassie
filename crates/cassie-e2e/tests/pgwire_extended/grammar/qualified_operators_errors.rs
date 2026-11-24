//! Unknown BYTEA inputs fail in Parse; explicit TEXT retains its own overload.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_bytea_input_errors_given_unknown_concat_literals_when_parse_coerces_operands() {
    // Arrange
    let cases = [
        ("SELECT '\\x00'::BYTEA || '\\xg1' AS v", "22023"),
        ("SELECT '\\xg1' || '\\x00'::BYTEA AS v", "22023"),
        ("SELECT '\\x00'::BYTEA || '\\x0' AS v", "22023"),
        ("SELECT '\\x0' || '\\x00'::BYTEA AS v", "22023"),
        ("SELECT '\\x00'::BYTEA || '\\q' AS v", "22P02"),
        ("SELECT '\\q' || '\\x00'::BYTEA AS v", "22P02"),
    ];
    let mut steps = Vec::new();
    for (index, (sql, _)) in cases.iter().enumerate() {
        steps.push(Step::ParseFlush(wire::parse_frame(
            &format!("invalid_{index}"),
            sql,
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
    }
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 7",
    )]));
    // Act
    let batches = run_script("bytea-concat-literal-errors", steps);
    // Assert
    for ((sql, state), group) in cases.iter().zip(batches.as_chunks::<3>().0) {
        for frames in [&group[0], &group[2]] {
            assert_eq!(
                wire::error_code(frames).as_deref(),
                Some(*state),
                "{sql}: {frames:?}"
            );
            assert!(!frames
                .iter()
                .any(|(tag, _)| matches!(tag, b'D' | b'C' | b'1')));
        }
        assert_eq!(
            group[0].iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'E']
        );
        assert_eq!(
            group[1].last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        assert_eq!(
            group[2].last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}
