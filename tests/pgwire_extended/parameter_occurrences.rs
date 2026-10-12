//! PostgreSQL 18.6 occurrence-order observations: Parse admission and retained metadata.

use super::support_parameter_occurrence_wire::{run_script, Step};
use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_reject_earlier_bare_unknown_occurrences_before_parse_publication() {
    // Arrange
    let cases = [
        ("SELECT $1 AS flag WHERE $1", "boolean versus text"),
        (
            "SELECT $1 AS a, CAST($1 AS BOOLEAN) AS b",
            "boolean versus text",
        ),
        ("SELECT $1 AS a, NOT $1 AS b", "boolean versus text"),
        (
            "SELECT $1 AS a, CAST($1 AS INT) AS b",
            "integer versus text",
        ),
        ("SELECT $1 AS value LIMIT $1", "bigint versus text"),
        (
            "SELECT $1 AS a, $1 AS b, NOT $1 AS c",
            "boolean versus text",
        ),
        (
            "SELECT $1 AS outer_value, EXISTS (SELECT 1 WHERE $1) AS gate",
            "boolean versus text",
        ),
        ("EXPLAIN SELECT $1 AS flag WHERE $1", "boolean versus text"),
    ];
    let mut steps = Vec::new();
    for (index, (sql, _)) in cases.iter().enumerate() {
        for oid in [0, 705] {
            let name = format!("rejected_{index}_{oid}");
            steps.push(Step::ParseFlush(wire::parse_frame_with_types(
                &name,
                sql,
                &[oid],
            )));
            // Describe is sent only after the observed Parse reply. A rejected
            // Parse must ignore it until Sync rather than publish metadata.
            steps.push(Step::ThroughReady(vec![
                wire::describe_statement_frame(&name),
                wire::sync_frame(),
            ]));
        }
    }

    // Act
    let batches = run_script("parameter-occurrence-rejection", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 4);
    for ((_, detail), pair_of_cases) in cases.into_iter().zip(batches.as_chunks::<4>().0) {
        for pair in pair_of_cases.as_chunks::<2>().0 {
            assert_eq!(pair[0].len(), 1, "one reply before any Sync");
            assert_error(
                &pair[0],
                "42P08",
                "inconsistent types deduced for parameter $1",
                Some(detail),
            );
            assert_eq!(tags(&pair[0]), vec![b'E']);
            assert_eq!(tags(&pair[1]), vec![b'Z']);
            assert_ready(&pair[1]);
        }
    }
}

struct PairCase {
    sql: &'static str,
    input: &'static [u8],
    parameter_oid: i32,
    columns: [(i32, i16); 2],
    text: [&'static [u8]; 2],
    binary: [&'static [u8]; 2],
}

#[test]
fn should_preserve_mixed_explicit_cast_outputs_after_occurrence_resolution() {
    // Arrange
    let cases = [
        PairCase {
            sql: "SELECT CAST($1 AS BOOLEAN) AS a, $1 AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (16, 1)],
            text: [b"t", b"t"],
            binary: [&[1], &[1]],
        },
        PairCase {
            sql: "SELECT NOT $1 AS a, $1 AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (16, 1)],
            text: [b"f", b"t"],
            binary: [&[0], &[1]],
        },
        PairCase {
            sql: "SELECT CAST($1 AS INT) AS a, $1 AS b",
            input: b"1",
            parameter_oid: 23,
            columns: [(23, 4), (23, 4)],
            text: [b"1", b"1"],
            binary: [&[0, 0, 0, 1], &[0, 0, 0, 1]],
        },
        PairCase {
            sql: "SELECT CAST($1 AS BIGINT) AS a, $1 AS b",
            input: b"1",
            parameter_oid: 20,
            columns: [(20, 8), (20, 8)],
            text: [b"1", b"1"],
            binary: [&[0, 0, 0, 0, 0, 0, 0, 1], &[0, 0, 0, 0, 0, 0, 0, 1]],
        },
        PairCase {
            sql: "SELECT NOT $1 AS a, CAST($1 AS BOOLEAN) AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (16, 1)],
            text: [b"f", b"t"],
            binary: [&[0], &[1]],
        },
        PairCase {
            sql: "SELECT CAST($1 AS BOOLEAN) AS a, CAST($1 AS INT) AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (23, 4)],
            text: [b"t", b"1"],
            binary: [&[1], &[0, 0, 0, 1]],
        },
        PairCase {
            sql: "SELECT CAST($1 AS BOOLEAN) AS a, $1 IS NULL AS b",
            input: b"true",
            parameter_oid: 16,
            columns: [(16, 1), (16, 1)],
            text: [b"t", b"f"],
            binary: [&[1], &[0]],
        },
    ];
    let mut requests = Vec::new();
    let mut cycles = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        for oid in [0, 705] {
            for format in [0, 1] {
                requests.push((index, oid, format));
                cycles.push(fixture::execute_cycle(
                    case.sql,
                    Some((oid, 0, Some(case.input))),
                    format,
                    true,
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-ordered-casts", &[], cycles);

    // Assert
    assert_eq!(batches.len(), requests.len());
    for ((index, oid, format), frames) in requests.into_iter().zip(batches) {
        let case = &cases[index];
        fixture::assert_success(&frames);
        assert_parameter_oids(&frames, &[case.parameter_oid]);
        assert_descriptors(&frames, &case.columns, &["a", "b"], format, true);
        let fields = if format == 0 { case.text } else { case.binary };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![row_payload(&fields)],
            "{}, supplied OID {oid}, result format {format}",
            case.sql
        );
        assert_command(&frames, b"SELECT 1\0");
    }
}

#[test]
fn should_preserve_declared_boolean_parameters_with_typed_nulls() {
    // Arrange
    let cases = [
        ("SELECT $1 AS flag WHERE $1", 16, Some(&[128_u8][..])),
        ("SELECT $1 AS flag WHERE $1", 16, None),
        ("SELECT CAST($1 AS BOOLEAN) AS flag WHERE $1", 0, None),
        ("SELECT CAST($1 AS BOOLEAN) AS flag WHERE $1", 705, None),
        ("SELECT CAST($1 AS BOOLEAN) AS flag WHERE $1", 16, None),
    ];
    let mut requests = Vec::new();
    let mut cycles = Vec::new();
    for (sql, oid, body) in cases {
        for format in [0, 1] {
            requests.push((body.is_some(), format));
            cycles.push(fixture::execute_cycle(
                sql,
                Some((oid, 1, body)),
                format,
                true,
            ));
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-typed-nulls", &[], cycles);

    // Assert
    assert_eq!(batches.len(), requests.len());
    for ((has_row, format), frames) in requests.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_parameter_oids(&frames, &[16]);
        fixture::assert_single_column_descriptors(&frames, (16, 1, -1), format, true);
        let expected = if has_row {
            let field = if format == 0 { &b"t"[..] } else { &[1][..] };
            vec![fixture::one_field_row(field)]
        } else {
            Vec::new()
        };
        assert_eq!(fixture::data_row_payloads(&frames), expected);
        assert_command(
            &frames,
            if has_row {
                b"SELECT 1\0"
            } else {
                b"SELECT 0\0"
            },
        );
    }
}

#[test]
fn should_reject_declared_text_at_where_without_retagging_it_as_boolean() {
    // Arrange
    let mut cycles = Vec::new();
    for body in [Some(&b"true"[..]), None] {
        for format in [0, 1] {
            cycles.push(fixture::execute_cycle(
                "SELECT $1 AS flag WHERE $1",
                Some((25, 0, body)),
                format,
                true,
            ));
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-known-text", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for frames in batches {
        assert_error(
            &frames,
            "42804",
            "argument of WHERE must be type boolean, not type text",
            None,
        );
        assert_eq!(tags(&frames), vec![b'E', b'Z']);
        assert_ready(&frames);
    }
}

#[test]
fn should_finalize_unknown_empty_output_as_text_before_describe() {
    // Arrange
    let cycles = [0, 705]
        .into_iter()
        .flat_map(|oid| {
            [0, 1].map(move |format| {
                fixture::execute_cycle(
                    "SELECT $1 AS value WHERE FALSE",
                    Some((oid, 0, Some(b"true"))),
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-empty-text", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for (format, frames) in [0, 1, 0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_parameter_oids(&frames, &[25]);
        fixture::assert_single_column_descriptors(&frames, (25, -1, -1), format, true);
        assert_eq!(fixture::data_row_payloads(&frames), Vec::<Vec<u8>>::new());
        assert_command(&frames, b"SELECT 0\0");
    }
}

#[test]
fn should_reject_an_unresolved_earlier_is_null_occurrence_before_parse_publication() {
    // Arrange
    let mut steps = Vec::new();
    for oid in [0, 705] {
        let name = format!("unresolved_null_{oid}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &name,
            "SELECT $1 IS NULL AS a, CAST($1 AS BOOLEAN) AS b",
            &[oid],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&name),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("parameter-occurrence-unresolved-is-null", steps);

    // Assert
    assert_eq!(batches.len(), 4);
    for pair in batches.as_chunks::<2>().0 {
        assert_error(
            &pair[0],
            "42P08",
            "could not determine data type of parameter $1",
            None,
        );
        assert_eq!(tags(&pair[0]), vec![b'E']);
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}

#[test]
fn should_resolve_unknown_pagination_parameter_types_from_offset_before_limit() {
    // Arrange
    let cases = [
        (
            "SELECT 1 LIMIT CAST($1 AS INT) OFFSET CAST($1 AS BIGINT)",
            20,
        ),
        (
            "SELECT 1 LIMIT CAST($1 AS BIGINT) OFFSET CAST($1 AS INT)",
            23,
        ),
    ];
    let mut requests = Vec::new();
    let mut cycles = Vec::new();
    for (sql, inferred) in cases {
        for oid in [0, 705, 23, 20] {
            for format in [0, 1] {
                requests.push((
                    if matches!(oid, 0 | 705) {
                        inferred
                    } else {
                        oid
                    },
                    format,
                ));
                cycles.push(fixture::execute_cycle(
                    sql,
                    Some((oid, 0, Some(b"0"))),
                    format,
                    true,
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-offset-first", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 16);
    for ((expected_oid, format), frames) in requests.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_parameter_oids(&frames, &[expected_oid]);
        // Unaliased expression naming is a separately retained descriptor gap.
        // This regression selects parameter ordering and numeric wire identity.
        fixture::assert_single_column_descriptors(&frames, (23, 4, -1), format, true);
        assert_eq!(fixture::data_row_payloads(&frames), Vec::<Vec<u8>>::new());
        assert_command(&frames, b"SELECT 0\0");
    }
}

#[test]
fn should_preserve_parameter_types_resolved_in_nested_sources_before_outer_targets() {
    // Arrange
    let statements = [
        "SELECT $1 AS outer_value FROM (SELECT CAST($1 AS BOOLEAN) AS inner_value) AS s WHERE $1",
        "WITH q AS (SELECT CAST($1 AS BOOLEAN) AS flag) SELECT $1 AS outer_value FROM q",
    ];
    let mut formats = Vec::new();
    let mut cycles = Vec::new();
    for sql in statements {
        for oid in [0, 705] {
            for format in [0, 1] {
                formats.push(format);
                cycles.push(fixture::execute_cycle(
                    sql,
                    Some((oid, 0, Some(b"true"))),
                    format,
                    true,
                ));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("parameter-occurrence-source-order", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 8);
    for (format, frames) in formats.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_parameter_oids(&frames, &[16]);
        assert_descriptors(&frames, &[(16, 1)], &["outer_value"], format, true);
        let field = if format == 0 { &b"t"[..] } else { &[1][..] };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(field)]
        );
        assert_command(&frames, b"SELECT 1\0");
    }
}

#[test]
fn should_reject_text_conjunction_operands_during_parse_in_either_order() {
    // Arrange
    let statements = [
        "SELECT $1 AND CAST($1 AS TEXT) AS result",
        "SELECT CAST($1 AS TEXT) AND $1 AS result",
    ];
    let mut steps = Vec::new();
    for (index, sql) in statements.into_iter().enumerate() {
        for oid in [0, 705, 16, 25] {
            let name = format!("and_text_{index}_{oid}");
            steps.push(Step::ParseFlush(wire::parse_frame_with_types(
                &name,
                sql,
                &[oid],
            )));
            steps.push(Step::ThroughReady(vec![
                wire::describe_statement_frame(&name),
                wire::sync_frame(),
            ]));
        }
    }

    // Act
    let batches = run_script("parameter-occurrence-and-order", steps);

    // Assert
    assert_eq!(batches.len(), 16);
    for pair in batches.as_chunks::<2>().0 {
        assert_error(
            &pair[0],
            "42804",
            "argument of AND must be type boolean, not type text",
            None,
        );
        assert_eq!(tags(&pair[0]), vec![b'E']);
        assert_eq!(tags(&pair[1]), vec![b'Z']);
        assert_ready(&pair[1]);
    }
}

#[test]
fn should_recover_named_reparse_across_parameter_rebindings() {
    // Arrange
    let inputs = [
        (Some(&[128_u8][..]), 1, true, None),
        (None, 1, false, None),
        (Some(&[0][..]), 1, false, None),
        (Some(&[0, 0][..]), 1, false, Some("22P03")),
        (Some(&[1][..]), 1, true, None),
        (Some(&b"true"[..]), 0, true, None),
    ];
    let mut steps = Vec::new();
    for oid in [0, 705] {
        let name = format!("recovery_{oid}");
        steps.extend([
            Step::ParseFlush(wire::parse_frame_with_types(
                &name,
                "SELECT $1 AS flag WHERE $1",
                &[oid],
            )),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ParseFlush(wire::parse_frame_with_types(
                &name,
                "SELECT $1 AS body, CAST($2 AS BOOLEAN) AS flag WHERE $2",
                &[25, 16],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(&name),
                wire::sync_frame(),
            ]),
        ]);
        for format in [0, 1] {
            for (index, (body, input_format, _, _)) in inputs.iter().enumerate() {
                let portal = format!("rebind_{oid}_{format}_{index}");
                steps.push(Step::ThroughReady(vec![
                    wire::describe_statement_frame(&name),
                    wire::bind_frame_with_formats(
                        &portal,
                        &name,
                        &[0, *input_format],
                        &[Some(b"kept"), *body],
                        &[format],
                    ),
                    wire::describe_portal_frame(&portal),
                    wire::execute_frame(&portal),
                    wire::sync_frame(),
                ]));
            }
        }
    }

    // Act
    let batches = run_script("parameter-occurrence-named-recovery", steps);

    // Assert
    assert_eq!(batches.len(), 32);
    for group in batches.as_chunks::<16>().0 {
        assert_error(
            &group[0],
            "42P08",
            "inconsistent types deduced for parameter $1",
            Some("boolean versus text"),
        );
        assert_eq!(tags(&group[0]), vec![b'E']);
        assert_eq!(tags(&group[1]), vec![b'Z']);
        assert_ready(&group[1]);
        assert_eq!(group[2], vec![(b'1', Vec::new())]);
        fixture::assert_success(&group[3]);
        assert_parameter_oids(&group[3], &[25, 16]);
        assert_descriptors(&group[3], &[(25, -1), (16, 1)], &["body", "flag"], 0, false);
        for (format, rebindings) in [0, 1].into_iter().zip(group[4..].as_chunks::<6>().0) {
            for ((_, _, has_row, error), frames) in inputs.into_iter().zip(rebindings) {
                assert_parameter_oids(frames, &[25, 16]);
                if let Some(state) = error {
                    assert_eq!(wire::error_code(frames).as_deref(), Some(state));
                    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
                    assert!(!frames
                        .iter()
                        .any(|(tag, _)| matches!(*tag, b'2' | b'D' | b'C')));
                    assert_descriptors(frames, &[(25, -1), (16, 1)], &["body", "flag"], 0, false);
                    assert_ready(frames);
                    continue;
                }
                fixture::assert_success(frames);
                assert_descriptors(
                    frames,
                    &[(25, -1), (16, 1)],
                    &["body", "flag"],
                    format,
                    true,
                );
                let expected = if has_row {
                    let flag = if format == 0 { &b"t"[..] } else { &[1][..] };
                    vec![row_payload(&[b"kept", flag])]
                } else {
                    Vec::new()
                };
                assert_eq!(fixture::data_row_payloads(frames), expected);
                assert_command(
                    frames,
                    if has_row {
                        b"SELECT 1\0"
                    } else {
                        b"SELECT 0\0"
                    },
                );
            }
        }
    }
}

fn tags(frames: &fixture::Frames) -> Vec<u8> {
    frames.iter().map(|(tag, _)| *tag).collect()
}

fn assert_ready(frames: &fixture::Frames) {
    assert_eq!(
        frames.last().map(|(tag, body)| (*tag, body.as_slice())),
        Some((b'Z', &b"I"[..]))
    );
}

fn assert_error(frames: &fixture::Frames, state: &str, message: &str, detail: Option<&str>) {
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    let (_, body) = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("ErrorResponse");
    let fields = wire::parse_error_fields(body);
    let field = |code| {
        fields
            .iter()
            .find(|(kind, _)| *kind == code)
            .map(|(_, text)| text.as_str())
    };
    assert_eq!(field('S'), Some("ERROR"));
    assert_eq!(field('C'), Some(state));
    assert_eq!(field('M'), Some(message));
    assert_eq!(field('D'), detail);
    assert_eq!(field('H'), None);
}

fn assert_parameter_oids(frames: &fixture::Frames, expected: &[i32]) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b't')
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 1);
    assert_eq!(
        wire::parse_parameter_description(&descriptions[0].1),
        expected
    );
}

fn assert_descriptors(
    frames: &fixture::Frames,
    expected: &[(i32, i16)],
    names: &[&str],
    format: i16,
    has_portal: bool,
) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), if has_portal { 2 } else { 1 });
    assert_eq!(expected.len(), names.len());
    for (index, columns) in descriptions.into_iter().enumerate() {
        assert_eq!(columns.len(), expected.len());
        for ((column, &(oid, size)), name) in columns.into_iter().zip(expected).zip(names) {
            assert_eq!(column.name, *name);
            assert_eq!((column.table_oid, column.attr_num), (0, 0));
            assert_eq!(
                (column.type_oid, column.type_size, column.type_mod),
                (oid, size, -1)
            );
            assert_eq!(column.format_code, if index == 0 { 0 } else { format });
        }
    }
}

fn row_payload(fields: &[&[u8]]) -> Vec<u8> {
    let mut payload = i16::try_from(fields.len())
        .expect("fixture field count")
        .to_be_bytes()
        .to_vec();
    for field in fields {
        payload.extend_from_slice(
            &i32::try_from(field.len())
                .expect("fixture field width")
                .to_be_bytes(),
        );
        payload.extend_from_slice(field);
    }
    payload
}

fn assert_command(frames: &fixture::Frames, expected: &[u8]) {
    let completions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'C')
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].1, expected);
}
