//! Native TEXT inputs converted to logical option arrays.
use crate::support_options_wire;
use crate::support_values_casts;

#[test]
fn should_expand_text_input_0_given_explicit_text_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[]) ORDER BY option_name,option_value NULLS LAST";
    let input = &[123, 97, 61, 98, 125];
    // Act
    let batches = support_values_casts::parameters("options-param-0", sql, 25, input, input);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_options_wire::assert_options(
        &batches,
        [
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_expand_text_input_1_given_explicit_text_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[]) ORDER BY option_name,option_value NULLS LAST";
    let input = &[123, 125];
    // Act
    let batches = support_values_casts::parameters("options-param-1", sql, 25, input, input);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_text_input_2_given_explicit_text_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[]) ORDER BY option_name,option_value NULLS LAST";
    let input = &[
        123, 119, 105, 116, 104, 111, 117, 116, 44, 97, 61, 98, 61, 99, 125,
    ];
    // Act
    let batches = support_values_casts::parameters("options-param-2", sql, 25, input, input);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
            ],
        ],
    );
}

#[test]
fn should_expand_text_input_3_given_explicit_text_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[]) ORDER BY option_name,option_value NULLS LAST";
    let input = &[123, 34, 97, 61, 34, 44, 34, 61, 118, 34, 125];
    // Act
    let batches = support_values_casts::parameters("options-param-3", sql, 25, input, input);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
            ],
            &[
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
            ],
        ],
    );
}

#[test]
fn should_expand_text_input_4_given_explicit_text_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[]) ORDER BY option_name,option_value NULLS LAST";
    let input = &[123, 97, 61, 49, 44, 97, 61, 50, 125];
    // Act
    let batches = support_values_casts::parameters("options-param-4", sql, 25, input, input);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
            ],
        ],
    );
}

#[test]
fn should_expand_text_input_5_given_explicit_text_parameters_when_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[]) ORDER BY option_name,option_value NULLS LAST";
    let input = &[123, 206, 148, 61, 228, 184, 150, 231, 149, 140, 125];
    // Act
    let batches = support_values_casts::parameters("options-param-5", sql, 25, input, input);
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    support_options_wire::assert_options(
        &batches,
        [
            &[&[
                0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
            ]],
            &[&[
                0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
            ]],
        ],
    );
}

#[test]
fn should_reuse_option_parameters_given_a_null_element_error_when_a_prepared_query_is_reexecuted() {
    use crate::support_parameter_occurrence_wire::{run_script, Step};
    use crate::support_pgwire as wire;
    use crate::support_type_metadata_contract as fixture;
    // Arrange
    let mut steps = Vec::new();
    for format in [0, 1] {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            "p",
            "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[])",
            &[25],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame("p"),
            wire::sync_frame(),
        ]));
        for input in [
            b"{a=first}".as_slice(),
            b"{NULL}".as_slice(),
            b"{a=last}".as_slice(),
        ] {
            steps.push(Step::BindFlush(wire::bind_frame_with_formats(
                "out",
                "p",
                &[0],
                &[Some(input)],
                &[format],
            )));
            steps.push(Step::ThroughReady(vec![
                wire::execute_frame("out"),
                wire::sync_frame(),
            ]));
        }
        steps.push(Step::ThroughReady(vec![
            wire::close_statement_frame("p"),
            wire::sync_frame(),
        ]));
    }
    // Act
    let batches = run_script("options-cached-error", steps);
    // Assert
    assert_eq!(batches.len(), 18);
    for cycle in batches.as_chunks::<9>().0 {
        assert_eq!(cycle[0][0].0, b'1');
        fixture::assert_success(&cycle[1]);
        for index in [2, 4, 6] {
            assert_eq!(cycle[index][0].0, b'2');
        }
        fixture::assert_success(&cycle[3]);
        assert_eq!(
            wire::data_rows(&cycle[3]),
            vec![vec![Some("a".into()), Some("first".into())]]
        );
        assert_eq!(wire::error_code(&cycle[5]).as_deref(), Some("22004"));
        assert_eq!(fixture::data_row_payloads(&cycle[5]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&cycle[7]);
        assert_eq!(
            wire::data_rows(&cycle[7]),
            vec![vec![Some("a".into()), Some("last".into())]]
        );
        fixture::assert_success(&cycle[8]);
    }
}

#[test]
fn should_reject_malformed_text_input_0_given_text_parameters_when_options_execute() {
    use crate::support_pgwire as wire;
    use crate::support_type_metadata_contract as fixture;
    // Arrange
    let input = &[123, 97, 61, 98];
    // Act
    let batches = support_values_casts::parameters(
        "options-invalid-0",
        "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[])",
        25,
        input,
        input,
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    for cycle in batches.as_chunks::<5>().0 {
        assert_eq!(cycle[2][0].0, b'2');
        assert_eq!(wire::error_code(&cycle[3]).as_deref(), Some("22P02"));
        assert_eq!(fixture::data_row_payloads(&cycle[3]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&cycle[4]);
    }
}

#[test]
fn should_reject_malformed_text_input_1_given_text_parameters_when_options_execute() {
    use crate::support_pgwire as wire;
    use crate::support_type_metadata_contract as fixture;
    // Arrange
    let input = &[123, 97, 61, 98, 125, 106, 117, 110, 107];
    // Act
    let batches = support_values_casts::parameters(
        "options-invalid-1",
        "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[])",
        25,
        input,
        input,
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    for cycle in batches.as_chunks::<5>().0 {
        assert_eq!(cycle[2][0].0, b'2');
        assert_eq!(wire::error_code(&cycle[3]).as_deref(), Some("22P02"));
        assert_eq!(fixture::data_row_payloads(&cycle[3]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&cycle[4]);
    }
}

#[test]
fn should_reject_malformed_text_input_2_given_text_parameters_when_options_execute() {
    use crate::support_pgwire as wire;
    use crate::support_type_metadata_contract as fixture;
    // Arrange
    let input = &[91, 34, 97, 61, 98, 34, 93];
    // Act
    let batches = support_values_casts::parameters(
        "options-invalid-2",
        "SELECT * FROM pg_options_to_table(($1::TEXT)::TEXT[])",
        25,
        input,
        input,
    );
    // Assert
    support_values_casts::assert_parameter(&batches, 25);
    for cycle in batches.as_chunks::<5>().0 {
        assert_eq!(cycle[2][0].0, b'2');
        assert_eq!(wire::error_code(&cycle[3]).as_deref(), Some("22P02"));
        assert_eq!(fixture::data_row_payloads(&cycle[3]), Vec::<Vec<u8>>::new());
        fixture::assert_success(&cycle[4]);
    }
}
