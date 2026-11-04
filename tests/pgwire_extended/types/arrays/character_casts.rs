//! Selected character-array conversions retain native identities and shape.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_apply_explicit_character_array_modifiers_without_losing_native_descriptors() {
    // Arrange
    let cases = [
        (
            "VALUES ('{x,NULL,abcd}'::CHAR(3)[])",
            1014,
            7,
            "{\"x  \",NULL,abc}",
        ),
        ("SELECT '{é猫z,NULL}'::VARCHAR(2)[]", 1015, 6, "{é猫,NULL}"),
        ("SELECT ARRAY[]::TEXT[]::CHAR(3)[]", 1014, 7, "{}"),
        (
            "SELECT '[0:0][-2:-1]={{é,NULL}}'::CHAR(3)[]",
            1014,
            7,
            "[0:0][-2:-1]={{\"é  \",NULL}}",
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, _, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-796-character-array-modifiers", &[], cycles);

    // Assert
    for ((sql, oid, typmod, expected), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, -1, typmod), 0, true);
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![Some(expected.to_owned())]],
            "{sql}"
        );
    }
}

#[test]
fn should_cast_native_array_parameters_to_character_elements_in_text_and_binary_formats() {
    // Arrange
    let sql = "SELECT $1::CHAR(3)[]";
    let input = "[-2:0]={é,NULL,long}";
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle(sql, Some((1009, 0, Some(input.as_bytes()))), format, true)
        })
        .to_vec();
    let mut binary = Vec::new();
    for value in [1_i32, 1, 1042, 3, -2] {
        binary.extend_from_slice(&value.to_be_bytes());
    }
    for value in [Some("é  "), None, Some("lon")] {
        binary.extend_from_slice(
            &value
                .map_or(-1, |value| {
                    i32::try_from(value.len()).expect("element length")
                })
                .to_be_bytes(),
        );
        if let Some(value) = value {
            binary.extend_from_slice(value.as_bytes());
        }
    }

    // Act
    let batches = fixture::run_cycles("issue-796-character-array-parameters", &[], cycles);

    // Assert
    for (format, frames) in [0, 1].into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (1014, -1, 7), format, true);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&parameters.1), vec![1009]);
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[-2:0]={\"é  \",NULL,lon}".to_owned())]]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(&binary)]
    );
}

#[test]
fn should_preserve_scalar_conversion_policy_for_native_character_array_elements() {
    // Arrange
    let cases = [
        ("SELECT ARRAY['x'::CHAR(3)]::TEXT[]", "{x}"),
        ("SELECT ARRAY['x'::CHAR(3)]::VARCHAR(3)[]", "{x}"),
        ("SELECT ARRAY[1234]::CHAR(3)[]", "{123}"),
        ("SELECT ARRAY[TRUE,FALSE]::CHAR(3)[]", "{tru,fal}"),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-796-character-array-scalar-policy", &[], cycles);

    // Assert
    for ((sql, expected), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![Some(expected.to_owned())]],
            "{sql}"
        );
    }
}

#[test]
fn should_reject_non_space_character_array_input_overflow_without_using_explicit_truncation() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle("SELECT pg_input_is_valid('{abcd}','CHAR(3)[]'),pg_input_is_valid('{abcd}','VARCHAR(3)[]'),pg_input_is_valid('{abc  }','VARCHAR(3)[]')", None, 0, true),
        fixture::execute_cycle("SELECT sql_error_code FROM pg_input_error_info('{abcd}','CHAR(3)[]')", None, 0, true),
        fixture::execute_cycle("SELECT 17", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("issue-796-character-array-input-policy", &[], cycles);

    // Assert
    for frames in &batches {
        fixture::assert_success(frames);
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("f".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned())
        ]]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("22001".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("17".to_owned())]]
    );
}

#[test]
fn should_reapply_character_array_casts_to_canonical_stored_elements_without_changing_bounds() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE character_array_casts (id INT PRIMARY KEY, items CHAR(3)[])",
            vec![],
        ),
        (
            "INSERT INTO character_array_casts VALUES (1,'[-2:-1]={x,NULL}'::CHAR(3)[])",
            vec![],
        ),
    ];
    let cycles = vec![fixture::execute_cycle(
        "SELECT items::CHAR(3)[] FROM character_array_casts",
        None,
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("issue-796-character-array-stored-cast", &setup, cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (1014, -1, 7), 0, true);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[-2:-1]={\"x  \",NULL}".to_owned())]]
    );
}

#[test]
fn should_keep_character_array_assignment_validation_distinct_from_explicit_casts() {
    // Arrange
    let setup = [(
        "CREATE TABLE character_array_assignment (id INT PRIMARY KEY, items VARCHAR(3)[])",
        vec![],
    )];
    let cycles = vec![
        fixture::execute_cycle(
            "INSERT INTO character_array_assignment VALUES (1,ARRAY['abcd'])",
            None,
            0,
            true,
        ),
        fixture::execute_cycle(
            "SELECT COUNT(*) FROM character_array_assignment",
            None,
            0,
            true,
        ),
        fixture::execute_cycle(
            "INSERT INTO character_array_assignment VALUES (1,ARRAY['abcd']::VARCHAR(3)[])",
            None,
            0,
            true,
        ),
        fixture::execute_cycle(
            "SELECT items::TEXT FROM character_array_assignment",
            None,
            0,
            true,
        ),
    ];

    // Act
    let batches = fixture::run_cycles(
        "issue-796-character-array-assignment-boundary",
        &setup,
        cycles,
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22001"));
    for frames in &batches[1..] {
        fixture::assert_success(frames);
    }
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("0".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("{abc}".to_owned())]]
    );
}

#[test]
fn should_preserve_multidimensional_binary_parameter_bounds_when_casting_character_elements() {
    // Arrange
    let mut input = Vec::new();
    let mut expected = Vec::new();
    for (bytes, oid, values) in [
        (
            &mut input,
            25_i32,
            [Some("é"), None, Some("long"), Some("x")],
        ),
        (
            &mut expected,
            1042,
            [Some("é  "), None, Some("lon"), Some("x  ")],
        ),
    ] {
        for value in [2_i32, 1, oid, 2, -1, 2, -2] {
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        for value in values {
            bytes.extend_from_slice(
                &value
                    .map_or(-1, |value| {
                        i32::try_from(value.len()).expect("element length")
                    })
                    .to_be_bytes(),
            );
            if let Some(value) = value {
                bytes.extend_from_slice(value.as_bytes());
            }
        }
    }
    let cycle = fixture::execute_cycle(
        "SELECT $1::CHAR(3)[]",
        Some((1009, 1, Some(&input))),
        1,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-796-character-array-multidimensional-binary",
        &[],
        vec![cycle],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (1014, -1, 7), 1, true);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(&expected)]
    );
}

#[test]
fn should_keep_scalar_and_array_character_temporal_casts_in_the_statement_timezone() {
    // Arrange
    let setup = [("SET TIME ZONE 'America/New_York'", vec![])];
    let sql = "SELECT ('2026-01-01 12:00:00+00'::TIMESTAMPTZ)::CHAR(32),ARRAY['2026-01-01 12:00:00+00'::TIMESTAMPTZ]::CHAR(32)[]";
    let expected = format!("{:32}", "2026-01-01 07:00:00-05");
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-796-character-array-temporal-session",
        &setup,
        vec![cycle],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some(expected.clone()),
            Some(format!("{{\"{expected}\"}}"))
        ]]
    );
}

#[test]
fn should_retain_explicit_array_modifiers_without_inheriting_them_from_function_arguments() {
    // Arrange
    let cases = [
        ("SELECT ARRAY['a'::VARCHAR(3)]", 7, "{a}"),
        (
            "SELECT COALESCE(ARRAY['a'::VARCHAR(3)],ARRAY['b'::VARCHAR(3)])",
            7,
            "{a}",
        ),
        ("SELECT array_fill('a'::VARCHAR(3),ARRAY[1])", -1, "{a}"),
        ("SELECT array_reverse(ARRAY['a'::VARCHAR(3)])", -1, "{a}"),
        (
            "SELECT array_agg(item) FROM (VALUES ('a'::VARCHAR(3))) s(item)",
            -1,
            "{a}",
        ),
        (
            "SELECT ARRAY['a'::VARCHAR(3)] || ARRAY['b'::VARCHAR(3)]",
            -1,
            "{a,b}",
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-796-character-array-call-modifiers", &[], cycles);

    // Assert
    for ((sql, modifier, expected), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (1015, -1, modifier), 0, true);
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![Some(expected.to_owned())]],
            "{sql}"
        );
    }
}
