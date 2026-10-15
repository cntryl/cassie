//! PostgreSQL 18 string-function parameter and result metadata tracked by #803.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_sql_fixture::{sql_fixture_with_config, SqlFixture};
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::AsyncWriteExt;

mod format_parameter_wires;
mod parse_ident_residuals;
mod strpos_result_identity;

fn configured_fixture(label: &str, maximum_rows: usize) -> (SqlFixture, CassieRuntimeConfig) {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = maximum_rows;
    let storage = sql_fixture_with_config(label, &[], config.clone());
    storage
        .cassie
        .startup()
        .expect("start set-returning function fixture");
    (storage, config)
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

async fn start_server(
    instance: Cassie,
    config: CassieRuntimeConfig,
) -> (SocketAddr, tokio::task::JoinHandle<Result<(), CassieError>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pgwire listener");
    let address = listener.local_addr().expect("listener address");
    drop(listener);
    let server = tokio::spawn(cassie::pgwire::server::run(
        address.to_string(),
        Arc::new(instance),
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    (address, server)
}

#[test]
fn should_bind_split_part_parameters_with_text_descriptor() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "split_part_parameters",
            "SELECT split_part($1, $2, $3)",
            &[25, 25, 23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("split_part_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "split_part_portal",
                "split_part_parameters",
                &["one,two,three", ",", "2"],
            ),
            wire::describe_portal_frame("split_part_portal"),
            wire::execute_frame("split_part_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-split-part-wire", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let parameter_types = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![25, 25, 23]
    );
    let result_columns = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_columns.1)[0].type_oid,
        25
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("two".to_owned())]]
    );
}

#[test]
fn should_report_invalid_parameter_value_for_zero_split_part_index() {
    // Arrange
    let batches = run_script(
        "issue-803-split-part-zero-index-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT split_part('a,b', ',', 1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT split_part('a,b', ',', 0)",
            )]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("a".to_owned())]]);
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22023"));
}

#[test]
fn should_report_null_value_not_allowed_for_null_identifier_format() {
    // Arrange
    let batches = run_script(
        "issue-803-format-null-identifier-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%I', 'identifier')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%I', NULL::TEXT)",
            )]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(
        wire::data_rows(valid),
        vec![vec![Some("identifier".to_owned())]]
    );
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22004"));
}

#[test]
fn should_preserve_left_text_parameter_wire_metadata() {
    // Arrange
    let sql = "SELECT left($1, $2) AS prefix";
    let text_bytes = b"abcdef";
    let count_bytes = 3_i32.to_be_bytes();
    let batches = run_script(
        "issue-803-left-parameter-wire-metadata",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types("left_text", sql, &[0, 0])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("left_text"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "left_binary",
                    "left_text",
                    &[1, 1],
                    &[Some(text_bytes), Some(&count_bytes)],
                    &[1],
                ),
                wire::describe_portal_frame("left_binary"),
                wire::execute_frame("left_binary"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|frame| frame.0 == b't')
                .expect("ParameterDescription")
                .1
        ),
        vec![25, 23]
    );
    let described_columns = wire::parse_row_description(
        &described
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("statement RowDescription")
            .1,
    );
    assert_eq!(described_columns[0].name, "prefix");
    assert_eq!(described_columns[0].type_oid, 25);
    let executed_columns = wire::parse_row_description(
        &executed
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("portal RowDescription")
            .1,
    );
    assert_eq!(executed_columns[0].format_code, 1);
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("abc".to_owned())]]
    );
}

#[test]
fn should_return_parse_ident_text_array_with_postgres_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-parse-ident-wire-result",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT parse_ident('\"Foo\".bar')",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("parse_ident RowDescription")
            .1,
    );
    assert_eq!(columns[0].type_oid, 1009);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("{Foo,bar}".to_owned())]]
    );
}

#[test]
fn should_return_string_to_table_rows_with_ordinality_metadata() {
    // Arrange
    let batches = run_script(
        "issue-803-string-to-table-wire-results",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT value, ordinal FROM string_to_table('a,,b', ',', '') WITH ORDINALITY AS t(value, ordinal) ORDER BY ordinal",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let columns = wire::parse_row_description(
        &result
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("string_to_table RowDescription")
            .1,
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("a".to_owned()), Some("1".to_owned())],
            vec![None, Some("2".to_owned())],
            vec![Some("b".to_owned()), Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_bind_string_to_table_parameters_with_postgres_result_metadata() {
    // Arrange
    let sql = "SELECT value, ordinal FROM string_to_table($1, $2, $3) WITH ORDINALITY AS t(value, ordinal) ORDER BY ordinal";
    let batches = run_script(
        "issue-803-string-to-table-parameters",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "string_to_table_parameters",
                sql,
                &[25, 25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("string_to_table_parameters"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "string_to_table_portal",
                    "string_to_table_parameters",
                    &["a,NULL,b", ",", "NULL"],
                ),
                wire::describe_portal_frame("string_to_table_portal"),
                wire::execute_frame("string_to_table_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("string_to_table ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 25, 25]
    );
    let columns = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("string_to_table portal RowDescription");
    assert_eq!(
        wire::parse_row_description(&columns.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 20]
    );
    assert_eq!(
        wire::data_rows(executed),
        vec![
            vec![Some("a".to_owned()), Some("1".to_owned())],
            vec![None, Some("2".to_owned())],
            vec![Some("b".to_owned()), Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_bind_ascii_name_plus_integer_overloads_after_describe() {
    // Arrange
    let batches = run_script(
        "issue-803-to-ascii-overload-wire-metadata",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "to_ascii_name",
                "SELECT to_ascii($1, $2) AS converted",
                &[25, 19],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("to_ascii_name"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("to_ascii_name_portal", "to_ascii_name", &["abc", "LATIN1"]),
                wire::describe_portal_frame("to_ascii_name_portal"),
                wire::execute_frame("to_ascii_name_portal"),
                wire::sync_frame(),
            ]),
            Step::ParseFlush(wire::parse_frame_with_types(
                "to_ascii_integer",
                "SELECT to_ascii($1, $2) AS converted",
                &[25, 23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("to_ascii_integer"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("to_ascii_integer_portal", "to_ascii_integer", &["abc", "8"]),
                wire::describe_portal_frame("to_ascii_integer_portal"),
                wire::execute_frame("to_ascii_integer_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let name_description = &batches[1];
    let name_result = &batches[2];
    let integer_description = &batches[4];
    let integer_result = &batches[5];

    // Assert
    for (parsed, described, executed, parameter_oids) in [
        (&batches[0], name_description, name_result, vec![25, 19]),
        (
            &batches[3],
            integer_description,
            integer_result,
            vec![25, 23],
        ),
    ] {
        assert_eq!(wire::error_code(parsed), None);
        assert_eq!(wire::error_code(described), None);
        assert_eq!(wire::error_code(executed), None);
        let parameters = described
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("to_ascii ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            parameter_oids
        );
        let columns = executed
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("to_ascii portal RowDescription");
        assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 25);
        assert_eq!(
            wire::data_rows(executed),
            vec![vec![Some("abc".to_owned())]]
        );
    }
}

#[test]
fn should_report_unsupported_utf8_to_ascii_conversion_with_0a000() {
    // Arrange
    let batches = run_script(
        "issue-803-to-ascii-utf8-sqlstate",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT to_ascii('abc')",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("0A000"));
}

#[test]
fn should_expand_multiple_unnest_arrays_in_lockstep_with_ordinality() {
    // Arrange
    let sql = "SELECT first_value, second_value, ordinal FROM unnest(ARRAY[10,20,30], ARRAY['a','b']) WITH ORDINALITY AS rows(first_value, second_value, ordinal) ORDER BY ordinal";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-unnest-lockstep-ordinality", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("UNNEST WITH ORDINALITY RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("10".to_owned()),
                Some("a".to_owned()),
                Some("1".to_owned())
            ],
            vec![
                Some("20".to_owned()),
                Some("b".to_owned()),
                Some("2".to_owned())
            ],
            vec![Some("30".to_owned()), None, Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_preserve_left_rows_when_lateral_unnest_is_empty() {
    // Arrange
    let sql = "SELECT source.id, expanded.value, expanded.ordinality FROM (VALUES (1, ARRAY[10,20]::int4[]), (2, ARRAY[]::int4[])) AS source(id, vals) LEFT JOIN LATERAL unnest(source.vals) WITH ORDINALITY AS expanded(value, ordinality) ON TRUE ORDER BY source.id, expanded.ordinality";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-left-join-empty-unnest", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("left lateral UNNEST RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("1".to_owned()),
                Some("10".to_owned()),
                Some("1".to_owned()),
            ],
            vec![
                Some("1".to_owned()),
                Some("20".to_owned()),
                Some("2".to_owned()),
            ],
            vec![Some("2".to_owned()), None, None],
        ]
    );
}

#[test]
fn should_expand_integer_generate_series_with_a_nonunit_step() {
    // Arrange
    let sql = "SELECT value FROM generate_series(2, 8, 3) AS series(value)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-generate-series-step", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("generate_series RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("2".to_owned())],
            vec![Some("5".to_owned())],
            vec![Some("8".to_owned())],
        ]
    );
}

#[test]
fn should_generate_array_subscripts_in_reverse_from_the_declared_lower_bound() {
    // Arrange
    let sql = "SELECT subscript FROM generate_subscripts('[0:2]={10,20,30}'::INT[], 1, TRUE) AS indexes(subscript)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-generate-subscripts-reverse", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("generate_subscripts RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("2".to_owned())],
            vec![Some("1".to_owned())],
            vec![Some("0".to_owned())],
        ]
    );
}

#[test]
fn should_zip_rows_from_functions_with_ordinality() {
    // Arrange
    let sql = "SELECT * FROM ROWS FROM (generate_series(1, 3), generate_series(10, 20, 10)) WITH ORDINALITY AS rows(first_value, second_value, ordinal)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-rows-from-ordinality", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("ROWS FROM RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("1".to_owned()),
                Some("10".to_owned()),
                Some("1".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("20".to_owned()),
                Some("2".to_owned()),
            ],
            vec![Some("3".to_owned()), None, Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_zip_select_list_set_returning_functions_to_the_longest_result() {
    // Arrange
    let sql =
        "SELECT generate_series(1, 3) AS first_value, generate_series(10, 20, 10) AS second_value";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-select-list-srf-lockstep", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("select-list SRF RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
            vec![Some("3".to_owned()), None],
        ]
    );
}

#[test]
fn should_execute_scalar_function_in_from_as_one_row_relation() {
    // Arrange
    let sql = "SELECT lowered FROM lower('HELLO') AS scalar_function(lowered)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-scalar-function-from", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("scalar function in FROM RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("hello".to_owned())]]
    );
}

#[test]
fn should_expand_numeric_generate_series_with_exact_steps() {
    // Arrange
    let sql = "SELECT generate_series(0.25, 1.0, 0.3)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-numeric-generate-series", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric generate_series RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1700
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("0.25".to_owned())],
            vec![Some("0.55".to_owned())],
            vec![Some("0.85".to_owned())],
        ]
    );
}

#[test]
fn should_advance_timestamptz_generate_series_by_local_days_across_dst() {
    // Arrange
    let sql = "SELECT generate_series(\
                  '2024-03-09 12:00:00-05', \
                  '2024-03-10 12:00:00-04', \
                  '1 day', 'America/New_York'\
               ) = '2024-03-10 12:00:00-04' OFFSET 1 LIMIT 1";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-timestamptz-series-timezone", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("time zone aware generate_series RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_bound_set_returning_results_without_publishing_partial_rows() {
    // Arrange
    let (storage, config) = configured_fixture("issue-808-srf-result-cap", 2);
    let runtime = current_thread_runtime();

    // Act
    let (overflow, bounded, recovery) = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        let overflow_queries = [
            "SELECT value FROM unnest(ARRAY[1,2,3]) AS expanded(value)",
            "SELECT * FROM ROWS FROM (generate_series(1,3), generate_series(10,30,10)) AS expanded(first_value, second_value)",
            "SELECT value FROM generate_series(1,3) AS expanded(value)",
        ];
        let mut overflow = Vec::with_capacity(overflow_queries.len());
        for query in overflow_queries {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(query)]).await;
            overflow.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }

        let bounded_queries = [
            "SELECT value FROM unnest(ARRAY[1,2,3]) AS expanded(value) LIMIT 2",
            "SELECT * FROM ROWS FROM (generate_series(1,3), generate_series(10,30,10)) AS expanded(first_value, second_value) LIMIT 2",
            "SELECT value FROM generate_series(1,10000) AS expanded(value) LIMIT 2",
        ];
        let mut bounded = Vec::with_capacity(bounded_queries.len());
        for query in bounded_queries {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(query)]).await;
            bounded.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 42")]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (overflow, bounded, recovery)
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(overflow.len(), 3);
    for frames in &overflow {
        assert_eq!(wire::error_code(frames).as_deref(), Some("54000"));
        assert!(wire::data_rows(frames).is_empty());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
        assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
        let error = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("result-cap ErrorResponse");
        let fields = wire::parse_error_fields(&error.1);
        assert_eq!(
            fields
                .iter()
                .find(|(field, _)| *field == 'M')
                .map(|(_, message)| message.as_str()),
            Some("query result row limit exceeded: 3 > 2")
        );
    }

    let expected_bounded = [
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]],
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
        ],
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]],
    ];
    assert_eq!(bounded.len(), expected_bounded.len());
    for (frames, rows) in bounded.iter().zip(expected_bounded) {
        assert_eq!(wire::error_code(frames), None);
        assert_eq!(wire::data_rows(frames), rows);
    }
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_resume_generate_series_portal_after_each_execute_row_limit() {
    // Arrange
    let (storage, config) = configured_fixture("issue-808-srf-portal-row-limit", 2);
    let runtime = current_thread_runtime();

    // Act
    let frames = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![
                wire::parse_frame(
                    "bounded_series_statement",
                    "SELECT value FROM generate_series(1,2) AS series(value)",
                ),
                wire::describe_statement_frame("bounded_series_statement"),
                wire::bind_frame("bounded_series_portal", "bounded_series_statement", &[]),
                wire::describe_portal_frame("bounded_series_portal"),
                wire::execute_limited_frame("bounded_series_portal", 1),
                wire::execute_limited_frame("bounded_series_portal", 1),
                wire::sync_frame(),
            ],
        )
        .await;
        let frames =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        frames
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(wire::error_code(&frames), None);
    let row_description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("generate_series portal RowDescription");
    assert_eq!(
        wire::parse_row_description(&row_description.1)[0].type_oid,
        23
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
    let suspended = frames
        .iter()
        .position(|(tag, _)| *tag == b's')
        .expect("first Execute suspends after one row");
    assert_eq!(
        frames[..suspended]
            .iter()
            .filter(|(tag, _)| *tag == b'D')
            .count(),
        1
    );
    assert_eq!(
        frames[suspended + 1..]
            .iter()
            .filter(|(tag, _)| *tag == b'D')
            .count(),
        1
    );
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 1);
    assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
}

#[test]
fn should_cancel_rows_from_set_expansion_without_partial_rows_or_stale_cancellation() {
    // Arrange
    let (storage, config) = configured_fixture("issue-808-srf-cancel", 3);
    let runtime = current_thread_runtime();

    // Act
    let (baseline, cancelled, recovery) = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = tokio::io::split(socket);
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        let baseline_query = "SELECT array_value, series_value FROM ROWS FROM (unnest(ARRAY[1,2,3]), generate_series(1,3)) AS expanded(array_value, series_value) WHERE series_value < 3 OR pg_sleep_for(INTERVAL '0 seconds') IS NOT NULL";
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(baseline_query)],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        let cancellable_query = "SELECT array_value, series_value FROM ROWS FROM (unnest(ARRAY[1,2,3]), generate_series(1,3)) AS expanded(array_value, series_value) WHERE series_value < 3 OR pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL";
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(cancellable_query)],
        )
        .await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        let mut cancel_socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 45")]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (baseline, cancelled, recovery)
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(wire::error_code(&baseline), None);
    assert_eq!(
        wire::data_rows(&baseline),
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), Some("2".to_owned())],
            vec![Some("3".to_owned()), Some("3".to_owned())],
        ]
    );
    assert_eq!(wire::error_code(&cancelled).as_deref(), Some("57014"));
    assert!(wire::data_rows(&cancelled).is_empty());
    assert_eq!(cancelled.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    assert_eq!(cancelled.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("45".to_owned())]]
    );
}

#[test]
fn should_use_explicit_timezone_for_timestamptz_series_across_dst() {
    // Arrange
    let query = "SELECT value FROM generate_series(\
        '2024-03-09 12:00:00-05'::TIMESTAMPTZ, \
        '2024-03-12 12:00:00-04'::TIMESTAMPTZ, \
        INTERVAL '1 day', 'America/New_York') AS series(value)";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(query)]),
    ];

    // Act
    let batches = run_script("issue-808-generate-series-explicit-timezone-dst", steps);

    // Assert
    let result = &batches[1];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("four-argument generate_series RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1_184
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("2024-03-09 17:00:00+00".to_owned())],
            vec![Some("2024-03-10 16:00:00+00".to_owned())],
            vec![Some("2024-03-11 16:00:00+00".to_owned())],
            vec![Some("2024-03-12 16:00:00+00".to_owned())],
        ]
    );
}
