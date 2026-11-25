//! Named-argument regex function binding through the PostgreSQL wire protocol.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn execute_text_parameterized_statement(
    fixture_name: &str,
    statement_name: &str,
    sql: &str,
    parameters: &[&str],
) -> Vec<Vec<(u8, Vec<u8>)>> {
    let portal_name = format!("{statement_name}_portal");
    let parameter_types = vec![0; parameters.len()];
    run_script(
        fixture_name,
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                statement_name,
                sql,
                &parameter_types,
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement_name),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(&portal_name, statement_name, parameters),
                wire::describe_portal_frame(&portal_name),
                wire::execute_frame(&portal_name),
                wire::sync_frame(),
            ]),
        ],
    )
}

#[test]
fn should_bind_named_regexp_like_arguments_over_pgwire() {
    // Arrange
    let sql = "SELECT regexp_like(string => $1, pattern => $2, flags => 'i') AS matched";
    let batches = run_script(
        "issue-799-regexp-like-named-parameters-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "named_regexp_like",
                sql,
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("named_regexp_like"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "named_regexp_like_portal",
                    "named_regexp_like",
                    &[0],
                    &[Some(b"Alpha"), Some(b"^alpha$")],
                    &[1],
                ),
                wire::describe_portal_frame("named_regexp_like_portal"),
                wire::execute_frame("named_regexp_like_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("named regexp_like ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25]
    );
    let statement_column = wire::parse_row_description(
        &described
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("named regexp_like statement RowDescription")
            .1,
    )
    .remove(0);
    assert_eq!(statement_column.type_oid, 16);
    assert_eq!(statement_column.type_size, 1);
    let portal_column = wire::parse_row_description(
        &executed
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("named regexp_like portal RowDescription")
            .1,
    )
    .remove(0);
    assert_eq!(portal_column.type_oid, 16);
    assert_eq!(portal_column.type_size, 1);
    assert_eq!(portal_column.format_code, 1);
    assert_eq!(
        executed
            .iter()
            .find(|frame| frame.0 == b'D')
            .expect("binary Boolean DataRow")
            .1,
        vec![0, 1, 0, 0, 0, 1, 1]
    );
}

#[test]
fn should_report_2201b_for_invalid_parameterized_regexp_like_flags_over_pgwire() {
    // Arrange
    let batches = execute_text_parameterized_statement(
        "issue-799-regexp-like-invalid-parameterized-flags-wire",
        "regexp_like_invalid_flags",
        "SELECT regexp_like($1, $2, $3) AS matched",
        &["value", "^value$", "z"],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed).as_deref(), Some("2201B"));
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("parameterized regexp_like ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25, 25]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("parameterized regexp_like RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        16
    );
}

#[test]
fn should_bind_parameterized_optional_regexp_like_flags_over_pgwire() {
    // Arrange
    let batches = execute_text_parameterized_statement(
        "issue-799-regexp-like-parameterized-flags-wire",
        "regexp_like_parameterized_flags",
        "SELECT regexp_like($1, $2, $3) AS matched",
        &["Alpha", "^alpha$", "i"],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("parameterized regexp_like ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25, 25]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("parameterized regexp_like RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        16
    );
    assert_eq!(wire::data_rows(executed), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_propagate_parameterized_null_flags_through_pgwire() {
    // Arrange
    let sql = "SELECT regexp_like($1, $2, $3) AS matched, \
               regexp_count($1, $2, $4, $3) AS matches";
    let batches = run_script(
        "issue-799-regexp-null-flags-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "regexp_null_flags",
                sql,
                &[0, 0, 0, 0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("regexp_null_flags"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "regexp_null_flags_portal",
                    "regexp_null_flags",
                    &[0],
                    &[Some(b"Alpha"), Some(b"^Alpha$"), None, Some(b"1")],
                    &[0],
                ),
                wire::describe_portal_frame("regexp_null_flags_portal"),
                wire::execute_frame("regexp_null_flags_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("parameterized regex ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25, 25, 23]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("parameterized regex RowDescription");
    let columns = wire::parse_row_description(&result_description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 23]
    );
    assert_eq!(wire::data_rows(executed), vec![vec![None, None]]);
}

#[test]
fn should_report_2201b_for_invalid_parameterized_regexp_count_flags_over_pgwire() {
    // Arrange
    let batches = execute_text_parameterized_statement(
        "issue-799-regexp-count-invalid-parameterized-flags-wire",
        "regexp_count_invalid_flags",
        "SELECT regexp_count($1, $2, $3, $4) AS matches",
        &["value", "value", "1", "z"],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed).as_deref(), Some("2201B"));
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("parameterized regexp_count ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25, 23, 25]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("parameterized regexp_count RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        23
    );
}

#[test]
fn should_accept_parameterized_named_regexp_count_syntax_over_pgwire() {
    // Arrange
    let sql =
        "SELECT regexp_count(string => $1, pattern => $2, start => $3, flags => $4) AS matches";
    let batches = run_script(
        "issue-799-regexp-count-named-parameters-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "named_regexp_count",
                sql,
                &[0, 0, 0, 0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("named_regexp_count"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "named_regexp_count_portal",
                    "named_regexp_count",
                    &["ABCABCAXYaxy", "A.", "1", "i"],
                ),
                wire::describe_portal_frame("named_regexp_count_portal"),
                wire::execute_frame("named_regexp_count_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(
        wire::error_code(parsed),
        None,
        "named regexp_count Parse errors: {:?}",
        parsed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(described),
        None,
        "named regexp_count Describe errors: {:?}",
        described
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(executed),
        None,
        "named regexp_count Bind/Execute errors: {:?}",
        executed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("named regexp_count ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25, 23, 25]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("named regexp_count RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        23
    );
    assert_eq!(wire::data_rows(executed), vec![vec![Some("4".to_owned())]]);
}

#[test]
fn should_infer_text_parameter_types_for_minimum_arity_regexp_like() {
    // Arrange
    let batches = execute_text_parameterized_statement(
        "issue-799-regexp-like-minimum-arity-wire",
        "regexp_like_minimum_arity",
        "SELECT regexp_like($1, $2) AS matched",
        &["Alpha", "^Alpha$"],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("regexp_like ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("regexp_like RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        16
    );
    assert_eq!(wire::data_rows(executed), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_bind_minimum_arity_regexp_count_parameters_over_pgwire() {
    // Arrange
    let batches = execute_text_parameterized_statement(
        "issue-799-regexp-count-minimum-arity-wire",
        "regexp_count_minimum_arity",
        "SELECT regexp_count($1, $2) AS matches",
        &["ABCA", "A"],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("regexp_count ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("regexp_count RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        23
    );
    assert_eq!(wire::data_rows(executed), vec![vec![Some("2".to_owned())]]);
}

#[test]
fn should_reject_named_regexp_count_flags_when_start_is_omitted_over_pgwire() {
    // Arrange
    let simple_sql = "SELECT regexp_count(string => 'abc', pattern => 'a', flags => 'i')";
    let extended_sql = "SELECT regexp_count(string => $1, pattern => $2, flags => 'i') AS matches";
    let batches = run_script(
        "issue-799-regexp-count-named-missing-start-wire",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(simple_sql)]),
            Step::ParseFlush(wire::parse_frame_with_types(
                "regexp_count_named_missing_start",
                extended_sql,
                &[0, 0],
            )),
            Step::ThroughReady(vec![wire::sync_frame()]),
        ],
    );

    // Act
    let simple = &batches[0];
    let parsed = &batches[1];

    // Assert
    assert_eq!(wire::error_code(simple).as_deref(), Some("42883"));
    assert_eq!(wire::error_code(parsed).as_deref(), Some("42883"));
    assert_eq!(batches[2].last().map(|frame| frame.0), Some(b'Z'));
}

#[test]
fn should_execute_positional_regexp_count_call_over_pgwire() {
    // Arrange
    let sql = "SELECT regexp_count($1, $2, $3, $4) AS matches";
    let batches = run_script(
        "issue-799-regexp-count-positional-parameters-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "positional_regexp_count",
                sql,
                &[25, 25, 23, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("positional_regexp_count"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "positional_regexp_count_portal",
                    "positional_regexp_count",
                    &["ABCABCAXYaxy", "A.", "1", "i"],
                ),
                wire::describe_portal_frame("positional_regexp_count_portal"),
                wire::execute_frame("positional_regexp_count_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(
        wire::error_code(parsed),
        None,
        "positional regexp_count Parse errors: {:?}",
        parsed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(described),
        None,
        "positional regexp_count Describe errors: {:?}",
        described
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(executed),
        None,
        "positional regexp_count Bind/Execute errors: {:?}",
        executed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("positional regexp_count ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25, 23, 25]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("positional regexp_count RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        23
    );
    assert_eq!(wire::data_rows(executed), vec![vec![Some("4".to_owned())]]);
}

#[test]
fn should_report_42883_for_unsupported_regex_function_signatures_over_pgwire() {
    // Arrange
    let cases = [
        "SELECT regexp_like('x')",
        "SELECT regexp_like('x', 'x', 'i', 'extra')",
        "SELECT regexp_count('x')",
        "SELECT regexp_count('x', 'x', 'i'::text)",
        "SELECT regexp_count('x', 'x', 1, 'i', 'extra')",
    ];
    let mut steps = Vec::with_capacity(cases.len() * 3);
    for (index, sql) in cases.iter().enumerate() {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &format!("invalid_regex_arity_{index}"),
            sql,
            &[],
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
    }

    // Act
    let batches = run_script("issue-799-regex-invalid-arity-wire", steps);

    // Assert
    for (sql, phases) in cases.iter().zip(batches.as_chunks::<3>().0) {
        assert_eq!(
            wire::error_code(&phases[0]).as_deref(),
            Some("42883"),
            "simple query: {sql}"
        );
        assert_eq!(
            wire::error_code(&phases[1]).as_deref(),
            Some("42883"),
            "Parse: {sql}"
        );
        assert_eq!(phases[2].last().map(|frame| frame.0), Some(b'Z'));
    }
}

#[test]
fn should_describe_qualified_regex_functions_over_pgwire() {
    // Arrange
    let like = execute_text_parameterized_statement(
        "issue-799-catalog-regexp-like-wire",
        "catalog_regexp_like",
        "SELECT pg_catalog.regexp_like(string => $1, pattern => $2, flags => 'i') AS matched",
        &["Alpha", "^alpha$"],
    );
    let count = execute_text_parameterized_statement(
        "issue-799-catalog-regexp-count-wire",
        "catalog_regexp_count",
        "SELECT pg_catalog.regexp_count(string => $1, pattern => $2, start => 1, flags => 'i') AS matches",
        &["ABCABC", "A."],
    );

    // Act
    let like_description = &like[1];
    let like_execution = &like[2];
    let count_description = &count[1];
    let count_execution = &count[2];

    // Assert
    assert_eq!(wire::error_code(&like[0]), None);
    assert_eq!(wire::error_code(like_description), None);
    assert_eq!(wire::error_code(like_execution), None);
    let like_parameters = like_description
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("qualified regexp_like ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&like_parameters.1),
        vec![25, 25]
    );
    let like_row = like_description
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("qualified regexp_like RowDescription");
    assert_eq!(wire::parse_row_description(&like_row.1)[0].type_oid, 16);
    assert_eq!(
        wire::data_rows(like_execution),
        vec![vec![Some("t".to_owned())]]
    );

    assert_eq!(wire::error_code(&count[0]), None);
    assert_eq!(wire::error_code(count_description), None);
    assert_eq!(wire::error_code(count_execution), None);
    let count_parameters = count_description
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("qualified regexp_count ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&count_parameters.1),
        vec![25, 25]
    );
    let count_row = count_description
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("qualified regexp_count RowDescription");
    assert_eq!(wire::parse_row_description(&count_row.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(count_execution),
        vec![vec![Some("2".to_owned())]]
    );
}
