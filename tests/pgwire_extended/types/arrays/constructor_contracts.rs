//! Pinned PostgreSQL18.6 array constructor shape, identities and temporal cells.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn hexadecimal(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("ASCII hex"), 16).expect("hex byte")
        })
        .collect()
}

#[test]
fn should_preserve_native_array_identity_for_multidimensional_constructors() {
    // Arrange
    let sql = "SELECT ARRAY[[1,NULL],[3,4]] AS items";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles("issue-807-rectangular-constructor", &[], cycles);

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (1_007, -1, -1), format, true);
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{{1,NULL},{3,4}}".to_owned())]]
    );
    assert_eq!(fixture::data_row_payloads(&batches[1]),vec![fixture::one_field_row(&hexadecimal("000000020000000100000017000000020000000100000002000000010000000400000001ffffffff00000004000000030000000400000004"))]);
}

#[test]
fn should_preserve_native_array_fill_shape_semantics() {
    // Arrange
    let sql = "SELECT array_fill(NULL::INT,ARRAY[2])::TEXT, array_fill(1,ARRAY[]::INT[])::TEXT, array_fill(1,ARRAY[0])::TEXT, array_ndims(array_fill(1,ARRAY[0]))";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-empty-null-fills", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("{NULL,NULL}".to_owned()),
            Some("{}".to_owned()),
            Some("{}".to_owned()),
            None
        ]]
    );
}

#[test]
fn should_preserve_native_temporal_array_elements_across_wire_formats() {
    // Arrange
    let cases = [
        (
            "SELECT ARRAY['01:02:03.100000'::TIME,NULL] AS items",
            1_183,
            "{01:02:03.1,NULL}",
            "00000001000000010000043b00000002000000010000000800000000dde9ff60ffffffff",
        ),
        (
            "SELECT ARRAY['2026-10-05 01:00:00+01:00'::TIMESTAMP,NULL] AS items",
            1_115,
            "{\"2026-10-05 01:00:00\",NULL}",
            "00000001000000010000045a0000000200000001000000080003000c2b86e400ffffffff",
        ),
        (
            "SELECT ARRAY['2026-10-05 01:00:00+01:00'::TIMESTAMPTZ,NULL] AS items",
            1_185,
            "{\"2026-10-05 00:00:00+00\",NULL}",
            "0000000100000001000004a00000000200000001000000080003000b54f34000ffffffff",
        ),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(sql, _, _, _)| {
            [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true))
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-807-native-temporal-arrays", &[], cycles);

    // Assert
    for ((_, oid, text, binary), pair) in cases.into_iter().zip(batches.chunks_exact(2)) {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (oid, -1, -1), format, true);
        }
        assert_eq!(wire::data_rows(&pair[0]), vec![vec![Some(text.to_owned())]]);
        assert_eq!(
            fixture::data_row_payloads(&pair[1]),
            vec![fixture::one_field_row(&hexadecimal(binary))]
        );
    }
}

#[test]
fn should_reject_nonrectangular_array_constructors_without_partial_output() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle("SELECT ARRAY[[1,2],[3]]", None, 0, true),
        fixture::execute_cycle("SELECT 1", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("issue-807-ragged-constructor", &[], cycles);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("2202E"));
    assert_eq!(
        batches[0]
            .iter()
            .filter(|(tag, _)| matches!(*tag, b'D' | b'C'))
            .count(),
        0
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("1".to_owned())]]
    );
}

#[test]
fn should_keep_array_constructor_subarrays_distinct_from_scalar_null_elements() {
    // Arrange
    let sql = "SELECT ARRAY[ARRAY[]::INT[],ARRAY[]::INT[]]::TEXT,ARRAY[NULL::INT[],NULL::INT[]]::TEXT,ARRAY[NULL::INT,NULL::INT]::TEXT";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-nested-empty-constructors", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("{}".to_owned()),
            Some("{}".to_owned()),
            Some("{NULL,NULL}".to_owned())
        ]]
    );
}

#[test]
fn should_bound_native_array_constructor_dimensions_at_the_selected_six() {
    // Arrange
    let cases = [
        ("SELECT ARRAY[[[[[[1]]]]]]", None),
        ("SELECT ARRAY[[[[[[[1]]]]]]]", Some("54000")),
        ("SELECT array_fill(1,ARRAY[1,1,1,1,1,1,1])", Some("54000")),
        ("SELECT ARRAY[ARRAY[1],ARRAY[]::INT[]]", Some("2202E")),
        ("SELECT ARRAY[]", Some("42P18")),
        ("SELECT ARRAY[]::INT[]", None),
        ("SELECT ARRAY[1,'2',NULL]", None),
        ("SELECT ARRAY[1,ARRAY[2]]", Some("42804")),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-807-dimension-limit", &[], cycles);

    // Assert
    for ((sql, state), frames) in cases.into_iter().zip(&batches) {
        assert_eq!(wire::error_code(frames).as_deref(), state, "{sql}");
        if state.is_some() {
            assert!(
                !frames.iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')),
                "{sql}"
            );
        }
    }
}

#[test]
fn should_format_native_temporal_arrays_under_the_connection_timezone() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle("SET TIME ZONE 'America/New_York'", None, 0, false),
        fixture::execute_cycle(
            "SELECT ARRAY['2026-01-01 00:00:00+00'::TIMESTAMPTZ,NULL] AS items",
            None,
            0,
            true,
        ),
        fixture::execute_cycle(
            "SELECT ARRAY['2026-01-01 00:00:00+00'::TIMESTAMPTZ,NULL]::TEXT AS items",
            None,
            0,
            true,
        ),
    ];

    // Act
    let batches = fixture::run_cycles("issue-807-array-session-timezone", &[], cycles);

    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
    }
    for frames in &batches[1..] {
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![Some("{\"2025-12-31 19:00:00-05\",NULL}".to_owned())]]
        );
    }
}

#[test]
fn should_resolve_unknown_array_elements_without_coercing_concrete_text_parameters() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle("SELECT ARRAY[1,'2',NULL] AS items", None, 0, true),
        fixture::execute_cycle(
            "SELECT ARRAY[$1,2] AS items",
            Some((0, 0, Some(b"1"))),
            0,
            true,
        ),
        fixture::execute_cycle(
            "SELECT ARRAY[$1,2] AS items",
            Some((25, 0, Some(b"1"))),
            0,
            true,
        ),
    ];

    // Act
    let batches = fixture::run_cycles("issue-807-array-common-input-types", &[], cycles);

    // Assert
    for frames in &batches[..2] {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (1_007, -1, -1), 0, true);
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{1,2,NULL}".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("{1,2}".to_owned())]]
    );
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("42804"));
    assert!(!batches[2]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'D' | b'C')));
}

#[test]
fn should_preserve_canonical_jsonb_array_elements_across_wire_formats() {
    // Arrange
    let sql = r#"SELECT ARRAY['{"b":2,"a":1e8}'::JSONB,NULL] AS items"#;
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    let canonical = r#"{"a": 100000000, "b": 2}"#;
    let mut binary = Vec::new();
    for value in [
        1_i32,
        1,
        3802,
        2,
        1,
        i32::try_from(canonical.len() + 1).expect("JSONB length"),
    ] {
        binary.extend_from_slice(&value.to_be_bytes());
    }
    binary.push(1);
    binary.extend_from_slice(canonical.as_bytes());
    binary.extend_from_slice(&(-1_i32).to_be_bytes());

    // Act
    let batches = fixture::run_cycles("issue-807-jsonb-array-native-format", &[], cycles);

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (3807, -1, -1), format, true);
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(
            r#"{"{\"a\": 100000000, \"b\": 2}",NULL}"#.to_owned()
        )]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(&binary)]
    );
}

#[test]
fn should_accumulate_native_array_subqueries_with_matching_declared_bounds() {
    // Arrange
    let sql = "SELECT ARRAY(SELECT items FROM (VALUES ('[-2:-1]={1,NULL}'::INT[]),('[-2:-1]={3,4}'::INT[])) s(items)) AS items";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-807-native-array-subquery", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[1:2][-2:-1]={{1,NULL},{3,4}}".to_owned())]]
    );
}

#[test]
fn should_use_visible_array_subquery_aliases_when_physical_names_collide() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY(SELECT column2 FROM (VALUES (ARRAY[1], 'x')) s(column2,column1))",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-subquery-alias-collision", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{{1}}".to_owned())]]
    );
}

#[test]
fn should_enforce_native_array_subquery_input_shape_requirements() {
    // Arrange
    let cases = [
        ("SELECT ARRAY(SELECT items FROM (VALUES (ARRAY[1]),(ARRAY[2,3]),(NULL::INT[])) s(items))","2202E"),
        ("SELECT ARRAY(SELECT items FROM (VALUES (ARRAY[1]),(ARRAY[[[[[[2]]]]]])) s(items))","2202E"),
        ("SELECT ARRAY(SELECT NULL::INT[])","22004"),
        ("SELECT ARRAY(SELECT ARRAY[]::INT[])","2202E"),
        ("SELECT ARRAY(SELECT items FROM (VALUES ('[0:0]={1}'::INT[]),('[1:1]={2}'::INT[])) s(items))","2202E"),
        ("SELECT ARRAY(SELECT ARRAY[[[[[[1]]]]]])","54000"),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(sql, _)| {
            [
                fixture::execute_cycle(sql, None, 0, true),
                fixture::execute_cycle("SELECT 9", None, 0, true),
            ]
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-807-array-subquery-errors", &[], cycles);

    // Assert
    for ((sql, state), pair) in cases.into_iter().zip(batches.chunks_exact(2)) {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some(state), "{sql}");
        assert!(
            !pair[0].iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')),
            "{sql}"
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("9".to_owned())]]);
    }
}
