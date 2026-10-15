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
                    assert_rebinding_error(frames, state);
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
