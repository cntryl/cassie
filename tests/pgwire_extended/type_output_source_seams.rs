//! Existing output names and identities constrain the private wire analyzer.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_describe_reserved_identity_on_a_base_table_with_declared_id() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE output_declared_identity (id INT, label TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO output_declared_identity (id, label) VALUES (37, 'stored')",
            Vec::new(),
        ),
    ];
    let statements = [
        "SELECT _id FROM output_declared_identity",
        "SELECT output_declared_identity._id FROM output_declared_identity",
        "SELECT public.output_declared_identity._id FROM output_declared_identity",
        "SELECT postgres.public.output_declared_identity._id FROM output_declared_identity",
    ];
    let cases = statements
        .into_iter()
        .flat_map(|sql| [false, true].map(|describe| (sql, describe)))
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(sql, describe)| fixture::execute_cycle(sql, None, 0, *describe))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-declared-id-reserved-identity", &setup, cycles);

    // Assert
    for ((sql, describe), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_columns(&frames, &[("_id", 25, -1, -1)], 0, describe);
        let rows = wire::data_rows(&frames);
        assert_eq!(rows.len(), 1, "{sql}");
        assert_eq!(rows[0].len(), 1, "{sql}");
        assert!(
            rows[0][0]
                .as_ref()
                .is_some_and(|identity| !identity.is_empty()),
            "{sql}"
        );
    }
}

#[test]
fn should_return_reserved_identity_when_insert_target_declares_id() {
    // Arrange
    let setup = [(
        "CREATE TABLE output_returned_identity (id INT, label TEXT)",
        Vec::new(),
    )];
    // Raw qualified RETURNING table._id is a separate pre-existing binder
    // rejection. Its supported expression form reaches output inference.
    let statements = [
        ("INSERT INTO output_returned_identity (id, label) VALUES (37, 'bare') RETURNING _id", "_id"),
        ("INSERT INTO output_returned_identity (id, label) VALUES (38, 'qualified') RETURNING COALESCE(output_returned_identity._id, NULL) AS identity", "identity"),
        ("INSERT INTO output_returned_identity (id, label) VALUES (39, 'qualified-schema') RETURNING COALESCE(public.output_returned_identity._id, NULL) AS identity", "identity"),
    ];
    let cases = statements
        .into_iter()
        .flat_map(|(sql, name)| [false, true].map(|describe| (sql, name, describe)))
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(sql, _, describe)| fixture::execute_cycle(sql, None, 0, *describe))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-returned-declared-id-identity", &setup, cycles);

    // Assert
    for ((sql, name, describe), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_columns(&frames, &[(name, 25, -1, -1)], 0, describe);
        let rows = wire::data_rows(&frames);
        assert_eq!(rows.len(), 1, "{sql}");
        assert_eq!(rows[0].len(), 1, "{sql}");
        assert!(
            rows[0][0]
                .as_ref()
                .is_some_and(|identity| !identity.is_empty()),
            "{sql}"
        );
    }
}

#[test]
fn should_preserve_wildcard_output_identity_in_scoped_sources() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE output_plain_source (label TEXT, n INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO output_plain_source (label, n) VALUES ('alpha', 7)",
            Vec::new(),
        ),
        (
            "CREATE TABLE output_real_id_source (id INT, label TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO output_real_id_source (id, label) VALUES (37, 'stored')",
            Vec::new(),
        ),
    ];
    let statements = [
        (
            "SELECT * FROM output_plain_source",
            vec![("id", 25, -1, -1), ("label", 25, -1, -1), ("n", 23, 4, -1)],
        ),
        (
            "WITH c AS (SELECT * FROM output_plain_source) SELECT * FROM c",
            vec![("id", 25, -1, -1), ("label", 25, -1, -1), ("n", 23, 4, -1)],
        ),
        (
            "SELECT * FROM (SELECT * FROM output_plain_source) AS d",
            vec![("id", 25, -1, -1), ("label", 25, -1, -1), ("n", 23, 4, -1)],
        ),
        (
            "WITH c AS (SELECT label, n FROM output_plain_source) SELECT * FROM c",
            vec![("label", 25, -1, -1), ("n", 23, 4, -1)],
        ),
        (
            "SELECT * FROM (SELECT label, n FROM output_plain_source) AS d",
            vec![("label", 25, -1, -1), ("n", 23, 4, -1)],
        ),
        (
            "SELECT * FROM output_real_id_source",
            vec![("id", 23, 4, -1), ("label", 25, -1, -1)],
        ),
        ("SELECT * FROM output_plain_source CROSS JOIN output_real_id_source", vec![("label", 25, -1, -1), ("n", 23, 4, -1), ("id", 23, 4, -1), ("label", 25, -1, -1)]),
        ("WITH c AS (SELECT * FROM output_plain_source) SELECT * FROM c CROSS JOIN output_real_id_source", vec![("label", 25, -1, -1), ("n", 23, 4, -1), ("id", 23, 4, -1), ("label", 25, -1, -1)]),
        ("SELECT * FROM (SELECT * FROM output_plain_source) AS d CROSS JOIN output_real_id_source", vec![("label", 25, -1, -1), ("n", 23, 4, -1), ("id", 23, 4, -1), ("label", 25, -1, -1)]),
    ];
    let cycles = statements
        .iter()
        .map(|(sql, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-wildcard-scoped-source", &setup, cycles);

    // Assert
    for ((sql, columns), frames) in statements.into_iter().zip(batches) {
        assert_eq!(wire::error_code(&frames), None, "{sql}");
        fixture::assert_success(&frames);
        assert_columns(&frames, &columns, 0, true);
        let rows = wire::data_rows(&frames);
        assert_eq!(rows.len(), 1, "{sql}");
        assert_eq!(rows[0].len(), columns.len(), "{sql}");
        if columns.len() == 4 {
            assert_eq!(
                rows,
                vec![vec![
                    Some("alpha".to_string()),
                    Some("7".to_string()),
                    Some("37".to_string()),
                    Some("stored".to_string())
                ]],
                "{sql}"
            );
        } else if columns[0].0 == "id" && columns[0].1 == 25 {
            assert!(
                rows[0][0]
                    .as_ref()
                    .is_some_and(|identity| !identity.is_empty()),
                "{sql}"
            );
            assert_eq!(
                &rows[0][1..],
                &[Some("alpha".to_string()), Some("7".to_string())],
                "{sql}"
            );
        } else if columns[0].0 == "id" {
            assert_eq!(
                rows,
                vec![vec![Some("37".to_string()), Some("stored".to_string())]],
                "{sql}"
            );
        } else {
            assert_eq!(
                rows,
                vec![vec![Some("alpha".to_string()), Some("7".to_string())]],
                "{sql}"
            );
        }
    }
}

#[test]
fn should_resolve_declared_id_without_a_derived_reserved_identity_alias() {
    // Arrange
    let setup = [
        ("CREATE TABLE output_identity_left (n INT)", Vec::new()),
        (
            "INSERT INTO output_identity_left (n) VALUES (7)",
            Vec::new(),
        ),
        ("CREATE TABLE output_identity_right (id INT)", Vec::new()),
        (
            "INSERT INTO output_identity_right (id) VALUES (37)",
            Vec::new(),
        ),
    ];
    let statements = [
        "SELECT id FROM (SELECT _id FROM output_identity_left) AS d CROSS JOIN output_identity_right",
        "WITH c AS (SELECT _id FROM output_identity_left) SELECT id FROM c CROSS JOIN output_identity_right",
    ];
    let cases = statements
        .into_iter()
        .flat_map(|sql| [0, 1].map(|format| (sql, format)))
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(sql, format)| fixture::execute_cycle(sql, None, *format, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-id-derived-shadowing", &setup, cycles);

    // Assert
    for ((sql, format), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_columns(&frames, &[("id", 23, 4, -1)], format, true);
        let expected = if format == 1 {
            fixture::one_field_row(&37_i32.to_be_bytes())
        } else {
            fixture::one_field_row(b"37")
        };
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected], "{sql}");
    }
}

#[test]
fn should_keep_qualified_same_name_output_types_distinct() {
    // Arrange
    let statements = [
        ("SELECT fixed_side.v AS chosen FROM (SELECT $1 AS v) AS numeric_side CROSS JOIN (SELECT 1 AS v) AS fixed_side", false),
        ("SELECT numeric_side.v AS chosen FROM (SELECT $1 AS v) AS numeric_side CROSS JOIN (SELECT 1 AS v) AS fixed_side", true),
    ];
    let cases = [700, 1700]
        .into_iter()
        .flat_map(|oid| statements.map(|(sql, rejected)| (oid, sql, rejected)))
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(oid, sql, _)| fixture::execute_cycle(sql, Some((*oid, 0, Some(b"2.5"))), 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-qualified-same-name-exports", &[], cycles);

    // Assert
    for ((oid, sql, rejected), frames) in cases.into_iter().zip(batches) {
        if rejected {
            fixture::assert_unsupported_before_metadata(&frames);
        } else {
            fixture::assert_success(&frames);
            assert_columns(&frames, &[("chosen", 23, 4, -1)], 0, true);
            assert_eq!(
                fixture::data_row_payloads(&frames),
                vec![fixture::one_field_row(b"1")],
                "{oid}: {sql}"
            );
            assert_parameter_oid(&frames, oid);
        }
    }
}

#[test]
fn should_accept_an_outer_cast_of_a_whole_set_with_numeric_output_origin() {
    // Arrange
    let sql = "SELECT CAST(v AS FLOAT) AS v FROM (SELECT 1 AS v UNION ALL SELECT $1 AS v) AS d";
    let inputs: [(Option<&[u8]>, Option<f64>); 3] = [
        (None, None),
        (Some(b"2"), Some(2.0)),
        (Some(b"2.5"), Some(2.5)),
    ];
    let mut cases = Vec::new();
    for oid in [700, 1700] {
        for format in [0, 1] {
            for (input, value) in inputs {
                cases.push((oid, format, input, value));
            }
        }
    }
    let cycles = cases
        .iter()
        .map(|(oid, format, input, _)| {
            fixture::execute_cycle(sql, Some((*oid, 0, *input)), *format, true)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-outer-cast-whole-set", &[], cycles);

    // Assert
    for ((oid, format, _, value), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_columns(&frames, &[("v", 701, 8, -1)], format, true);
        assert_parameter_oid(&frames, oid);
        let first = if format == 1 {
            fixture::one_field_row(&1.0_f64.to_be_bytes())
        } else {
            fixture::one_field_row(b"1")
        };
        let second = match value {
            None => vec![0, 1, 255, 255, 255, 255],
            Some(value) if format == 1 => fixture::one_field_row(&value.to_be_bytes()),
            Some(value) => fixture::one_field_row(value.to_string().as_bytes()),
        };
        let mut expected = vec![first, second];
        expected.sort();
        let mut actual = fixture::data_row_payloads(&frames);
        actual.sort();
        assert_eq!(actual, expected);
    }
}

#[test]
fn should_preserve_fixed_left_metadata_for_noncontributing_numeric_inputs() {
    // Arrange
    let inputs: [(Option<&[u8]>, bool); 3] =
        [(None, false), (Some(b"1"), true), (Some(b"2.5"), false)];
    let mut cases = Vec::new();
    for oid in [700, 1700] {
        for format in [0, 1] {
            for (sql, intersect) in [
                ("SELECT 1 AS v EXCEPT SELECT $1 AS v", false),
                ("SELECT 1 AS v INTERSECT SELECT $1 AS v", true),
            ] {
                for (input, matches_left) in inputs {
                    cases.push((oid, format, sql, input, intersect == matches_left));
                }
            }
        }
    }
    let cycles = cases
        .iter()
        .map(|(oid, format, sql, input, _)| {
            fixture::execute_cycle(sql, Some((*oid, 0, *input)), *format, true)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-fixed-left-set-metadata", &[], cycles);

    // Assert
    for ((oid, format, _, _, emits_row), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_columns(&frames, &[("v", 23, 4, -1)], format, true);
        assert_parameter_oid(&frames, oid);
        let expected = if emits_row {
            vec![if format == 1 {
                fixture::one_field_row(&1_i32.to_be_bytes())
            } else {
                fixture::one_field_row(b"1")
            }]
        } else {
            Vec::new()
        };
        assert_eq!(fixture::data_row_payloads(&frames), expected);
    }
}

fn assert_columns(
    frames: &fixture::Frames,
    expected: &[(&str, i32, i16, i32)],
    format: i16,
    described: bool,
) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), if described { 2 } else { 1 });
    for (index, actual) in descriptions.into_iter().enumerate() {
        assert_eq!(actual.len(), expected.len());
        for (actual, (name, oid, size, modifier)) in actual.iter().zip(expected) {
            assert_eq!(
                (
                    actual.name.as_str(),
                    actual.type_oid,
                    actual.type_size,
                    actual.type_mod
                ),
                (*name, *oid, *size, *modifier)
            );
            assert_eq!(
                actual.format_code,
                if described && index == 0 { 0 } else { format }
            );
        }
    }
}

fn assert_parameter_oid(frames: &fixture::Frames, expected: i32) {
    let (_, payload) = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(payload), vec![expected]);
}

#[test]
fn should_preserve_dml_wildcard_identity_names_across_wire_surfaces() {
    // Arrange
    let setup = [(
        "CREATE TABLE output_returning_wildcard (label TEXT, n INT)",
        Vec::new(),
    )];
    let cases = [
        (
            "INSERT INTO output_returning_wildcard (label, n) VALUES ('alpha', 7) RETURNING *",
            "alpha",
            "7",
            true,
            "_id",
        ),
        (
            "INSERT INTO output_returning_wildcard (label, n) VALUES ('beta', 8) RETURNING *",
            "beta",
            "8",
            false,
            "_id",
        ),
        (
            "SELECT * FROM output_returning_wildcard WHERE label = 'alpha'",
            "alpha",
            "7",
            true,
            "id",
        ),
        (
            "SELECT * FROM output_returning_wildcard WHERE label = 'alpha'",
            "alpha",
            "7",
            false,
            "id",
        ),
        (
            "UPDATE output_returning_wildcard SET n = 9 WHERE label = 'alpha' RETURNING *",
            "alpha",
            "9",
            true,
            "_id",
        ),
        (
            "UPDATE output_returning_wildcard SET n = 10 WHERE label = 'beta' RETURNING *",
            "beta",
            "10",
            false,
            "_id",
        ),
        (
            "DELETE FROM output_returning_wildcard WHERE label = 'alpha' RETURNING *",
            "alpha",
            "9",
            true,
            "_id",
        ),
        (
            "DELETE FROM output_returning_wildcard WHERE label = 'beta' RETURNING *",
            "beta",
            "10",
            false,
            "_id",
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, _, describe, _)| fixture::execute_cycle(sql, None, 0, *describe))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-returning-wildcard-name", &setup, cycles);

    // Assert
    let mut identities = std::collections::HashMap::new();
    for ((sql, label, number, describe, identity_name), frames) in cases.into_iter().zip(batches) {
        assert_eq!(wire::error_code(&frames), None, "{sql}");
        fixture::assert_success(&frames);
        assert_columns(
            &frames,
            &[
                (identity_name, 25, -1, -1),
                ("label", 25, -1, -1),
                ("n", 23, 4, -1),
            ],
            0,
            describe,
        );
        let rows = wire::data_rows(&frames);
        assert_eq!(rows.len(), 1, "{sql}");
        assert_eq!(rows[0].len(), 3, "{sql}");
        let identity = rows[0][0].as_ref().expect("returned internal identity");
        assert!(!identity.is_empty(), "{sql}");
        if let Some(previous) = identities.get(label) {
            assert_eq!(identity, previous, "{sql}");
        } else {
            identities.insert(label, identity.clone());
        }
        assert_eq!(
            &rows[0][1..],
            &[Some(label.to_string()), Some(number.to_string())],
            "{sql}"
        );
    }
}

// Append to the existing pgwire_type_output_source_seams child. Its private
// assert_columns helper and wire/fixture imports remain the owners.

#[test]
fn should_preserve_reserved_identity_with_predicate_only_parameters() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE output_parameter_identity (id INT, label TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO output_parameter_identity (id, label) VALUES (37, 'stored')",
            Vec::new(),
        ),
    ];
    let mut cycles = vec![fixture::execute_cycle(
        "SELECT _id FROM output_parameter_identity",
        None,
        0,
        true,
    )];
    let cases = [
        (&b"true"[..], false, true),
        (&b"false"[..], false, false),
        (&b"true"[..], true, true),
        (&b"false"[..], true, false),
    ];
    cycles.extend(cases.into_iter().map(|(body, describe, _)| {
        fixture::execute_cycle(
            "SELECT _id FROM output_parameter_identity WHERE $1",
            Some((16, 0, Some(body))),
            0,
            describe,
        )
    }));

    // Act
    let batches = fixture::run_cycles("type-output-parameter-identity", &setup, cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_columns(&batches[0], &[("_id", 25, -1, -1)], 0, true);
    let baseline = wire::data_rows(&batches[0]);
    assert_eq!(baseline.len(), 1);
    assert!(baseline[0][0]
        .as_ref()
        .is_some_and(|value| !value.is_empty()));
    for ((_, describe, includes_row), frames) in cases.into_iter().zip(&batches[1..]) {
        fixture::assert_success(frames);
        assert_columns(frames, &[("_id", 25, -1, -1)], 0, describe);
        assert_eq!(
            wire::data_rows(frames),
            if includes_row {
                baseline.clone()
            } else {
                Vec::<Vec<Option<String>>>::new()
            }
        );
    }
}

#[test]
fn should_preserve_declared_id_metadata_after_parameterized_identity_exports() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE output_parameter_identity_left (n INT)",
            Vec::new(),
        ),
        (
            "CREATE TABLE output_parameter_identity_right (id INT)",
            Vec::new(),
        ),
    ];
    let statements = [
        (
            "SELECT id FROM (SELECT _id FROM output_parameter_identity_left) AS d CROSS JOIN output_parameter_identity_right",
            &[][..],
            Vec::new(),
        ),
        (
            "SELECT id FROM (SELECT _id FROM output_parameter_identity_left WHERE $1) AS d CROSS JOIN output_parameter_identity_right",
            &[16][..],
            vec![Some(&b"true"[..])],
        ),
        (
            "WITH c AS (SELECT _id FROM output_parameter_identity_left WHERE $1) SELECT id FROM c CROSS JOIN output_parameter_identity_right",
            &[16][..],
            vec![Some(&b"false"[..])],
        ),
    ];
    let cycles = statements
        .into_iter()
        .map(|(sql, oids, parameters)| {
            vec![
                wire::parse_frame_with_types("", sql, oids),
                wire::describe_statement_frame(""),
                wire::bind_frame_with_formats("", "", &[], &parameters, &[]),
                wire::describe_portal_frame(""),
                wire::sync_frame(),
            ]
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-parameter-derived-id", &setup, cycles);

    // Assert
    // These are metadata-only cycles. Derived runtime row lookup is separately
    // owned by #761; this oracle cannot accidentally claim that owner closed.
    for frames in batches {
        fixture::assert_success(&frames);
        assert_columns(&frames, &[("id", 23, 4, -1)], 0, true);
        assert_eq!(fixture::data_row_payloads(&frames), Vec::<Vec<u8>>::new());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    }
}

#[test]
fn should_preserve_existing_dml_identity_names_when_an_alias_is_named_id() {
    // Arrange
    let setup = [(
        "CREATE TABLE output_returning_id_alias (label TEXT, n INT)",
        Vec::new(),
    )];
    let statements = [
        ("INSERT INTO output_returning_id_alias (label, n) VALUES ('alpha', 7) RETURNING label AS id, *", None),
        ("INSERT INTO output_returning_id_alias (label, n) SELECT 'alpha', 7 WHERE $1 RETURNING label AS id, *", Some((16, 0, Some(&b"true"[..])))),
    ];
    let cases = statements
        .into_iter()
        .flat_map(|(sql, parameter)| {
            [false, true]
                .into_iter()
                .flat_map(move |describe| [0, 1].map(|format| (sql, parameter, describe, format)))
        })
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(sql, parameter, describe, format)| {
            fixture::execute_cycle(sql, *parameter, *format, *describe)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-returning-id-alias", &setup, cycles);

    // Assert
    for ((sql, parameter, describe, format), frames) in cases.into_iter().zip(batches) {
        assert_eq!(wire::error_code(&frames), None, "{sql}");
        fixture::assert_success(&frames);
        assert_columns(
            &frames,
            &[
                ("_id", 25, -1, -1),
                ("id", 25, -1, -1),
                ("label", 25, -1, -1),
                ("n", 23, 4, -1),
            ],
            format,
            describe,
        );
        if parameter.is_some() && describe {
            assert_parameter_oid(&frames, 16);
        }
        let rows = wire::data_rows(&frames);
        assert_eq!(rows.len(), 1, "{sql}");
        assert_eq!(rows[0].len(), 4, "{sql}");
        assert_eq!(rows[0][0].as_deref(), Some("alpha"), "{sql}");
        let identity = rows[0][1].as_ref().expect("wildcard identity");
        assert!(!identity.is_empty(), "{sql}");
        let number = if format == 1 {
            &b"\0\0\0\x07"[..]
        } else {
            &b"7"[..]
        };
        let expected = literal_output_payload(&[
            Some(b"alpha"),
            Some(identity.as_bytes()),
            Some(b"alpha"),
            Some(number),
        ]);
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected], "{sql}");
    }
}

#[test]
fn should_preserve_dml_wildcard_results_with_boolean_parameters() {
    // Arrange
    let setup = [(
        "CREATE TABLE output_returning_bool (label TEXT, gate BOOLEAN, n INT)",
        Vec::new(),
    )];
    let sql = "INSERT INTO output_returning_bool (label, gate, n) VALUES ('parameter', $1, 7) RETURNING *";
    let inputs = [
        (0, Some(&b"true"[..]), Some(true)),
        (1, Some(&b"\0"[..]), Some(false)),
        (0, None, None),
    ];
    let cases = inputs
        .into_iter()
        .flat_map(|(input_format, input, value)| {
            [false, true].into_iter().flat_map(move |describe| {
                [0, 1].map(|format| (input_format, input, value, describe, format))
            })
        })
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(input_format, input, _, describe, format)| {
            fixture::execute_cycle(sql, Some((16, *input_format, *input)), *format, *describe)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-output-returning-boolean-parameter", &setup, cycles);

    // Assert
    for ((_, _, value, describe, format), frames) in cases.into_iter().zip(batches) {
        assert_eq!(wire::error_code(&frames), None, "{sql}");
        fixture::assert_success(&frames);
        assert_columns(
            &frames,
            &[
                ("_id", 25, -1, -1),
                ("label", 25, -1, -1),
                ("gate", 16, 1, -1),
                ("n", 23, 4, -1),
            ],
            format,
            describe,
        );
        if describe {
            assert_parameter_oid(&frames, 16);
        }
        let rows = wire::data_rows(&frames);
        assert_eq!(rows.len(), 1, "{sql}");
        assert_eq!(rows[0].len(), 4, "{sql}");
        let identity = rows[0][0].as_ref().expect("wildcard identity");
        assert!(!identity.is_empty(), "{sql}");
        let gate = match (value, format) {
            (Some(true), 1) => Some(&b"\x01"[..]),
            (Some(false), 1) => Some(&b"\0"[..]),
            (Some(true), _) => Some(&b"t"[..]),
            (Some(false), _) => Some(&b"f"[..]),
            (None, _) => None,
        };
        let number = if format == 1 {
            &b"\0\0\0\x07"[..]
        } else {
            &b"7"[..]
        };
        let expected = literal_output_payload(&[
            Some(identity.as_bytes()),
            Some(b"parameter"),
            gate,
            Some(number),
        ]);
        assert_eq!(fixture::data_row_payloads(&frames), vec![expected], "{sql}");
    }
}

fn literal_output_payload(fields: &[Option<&[u8]>]) -> Vec<u8> {
    let mut payload = i16::try_from(fields.len())
        .expect("fixed fixture width")
        .to_be_bytes()
        .to_vec();
    for field in fields {
        match field {
            Some(field) => {
                payload.extend_from_slice(
                    &i32::try_from(field.len())
                        .expect("fixed fixture field")
                        .to_be_bytes(),
                );
                payload.extend_from_slice(field);
            }
            None => payload.extend_from_slice(&(-1_i32).to_be_bytes()),
        }
    }
    payload
}

#[test]
fn should_preserve_no_data_for_dml_without_returning() {
    // Arrange
    let setup = [("CREATE TABLE output_no_returning (gate BOOLEAN)", vec![])];
    let commands = [
        (
            "INSERT INTO output_no_returning (gate) VALUES (TRUE)",
            "INSERT INTO output_no_returning (gate) VALUES ($1)",
        ),
        (
            "UPDATE output_no_returning SET gate=FALSE WHERE FALSE",
            "UPDATE output_no_returning SET gate=$1 WHERE FALSE",
        ),
        (
            "DELETE FROM output_no_returning WHERE FALSE",
            "DELETE FROM output_no_returning WHERE $1 AND FALSE",
        ),
    ];
    let cases = commands
        .into_iter()
        .flat_map(|(ordinary, parameterized)| {
            [false, true].into_iter().flat_map(move |parameter| {
                [false, true].into_iter().flat_map(move |describe| {
                    [0, 1].map(move |format| {
                        (
                            if parameter { parameterized } else { ordinary },
                            parameter,
                            describe,
                            format,
                        )
                    })
                })
            })
        })
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(sql, parameter, describe, format)| {
            fixture::execute_cycle(
                sql,
                parameter.then_some((16, 0, Some(&b"true"[..]))),
                *format,
                *describe,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-no-returning-command-metadata", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 24);
    for ((sql, _, describe, _), frames) in cases.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        assert_eq!(
            frames
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D'))
                .count(),
            0,
            "{sql}"
        );
        assert_eq!(
            frames.iter().filter(|(tag, _)| *tag == b'n').count(),
            if describe { 2 } else { 0 },
            "{sql}"
        );
        assert_eq!(
            frames.iter().filter(|(tag, _)| *tag == b'C').count(),
            1,
            "{sql}"
        );
    }
}

#[test]
fn should_preserve_execution_owned_projection_verification_output() {
    // Arrange
    let setup = [("CREATE TABLE output_verification (n INT)", vec![])];
    let sql = "VERIFY PROJECTION output_verification MODE metadata_only";
    let cycles = vec![
        vec![wire::simple_query_frame(sql)],
        fixture::execute_cycle(sql, None, 0, false),
        fixture::execute_cycle(sql, None, 1, false),
    ];
    let expected = [
        ("state", 25, -1, -1),
        ("target_collection", 25, -1, -1),
        ("mode", 25, -1, -1),
        ("mismatch_count", 20, 8, -1),
        ("missing_count", 20, 8, -1),
        ("stale_count", 20, 8, -1),
        ("repairable", 16, 1, -1),
        ("checked_components", 25, -1, -1),
        ("skipped_components", 25, -1, -1),
        ("last_error", 25, -1, -1),
    ];

    // Act
    let batches = fixture::run_cycles("type-execution-owned-verification", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 3);
    for (index, frames) in batches.into_iter().enumerate() {
        fixture::assert_success(&frames);
        let format = i16::from(index == 2);
        assert_columns(&frames, &expected, format, false);
        let rows = wire::data_rows(&frames);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 10);
        assert_eq!(rows[0][2].as_deref(), Some("metadata_only"));
        if format == 1 {
            let payloads = fixture::data_row_payloads(&frames);
            assert_eq!(&payloads[0][..2], &[0, 10]);
            let mut remaining = &payloads[0][2..];
            let mut fields = Vec::new();
            for _ in 0..10 {
                let (length, tail) = remaining.split_at(4);
                let length =
                    usize::try_from(i32::from_be_bytes(length.try_into().expect("field length")))
                        .expect("verification fields are non-NULL");
                let (value, tail) = tail.split_at(length);
                fields.push(value);
                remaining = tail;
            }
            assert_eq!(remaining, b"");
            assert_eq!(fields[2], b"metadata_only");
            for field in &fields[3..6] {
                assert_eq!(field.len(), 8);
            }
            assert!(matches!(fields[6], [0 | 1]));
        }
    }
}
