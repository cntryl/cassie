//! PostgreSQL 18 parser, relation, collation, and wire contracts tracked by #796 and #800.

use std::time::Duration;

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;
use tokio::io::AsyncWriteExt;

mod row_ordering_null_stopping;
mod values_typed_source_descriptors;

struct GrammarFixtureDirectory(String);

impl Drop for GrammarFixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_describe_postgres_cast_parameter_oid_through_extended_protocol() {
    // Arrange
    let batches = run_script(
        "issue-796-cast-parameter-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "cast_parameter",
                "SELECT $1::DOUBLE PRECISION AS value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("cast_parameter"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(described), None);
    let parameter_types = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![701]
    );
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 701);
}

#[test]
fn should_parse_digit_separators_as_integer_literal_syntax() {
    // Arrange
    let batches = run_script(
        "issue-796-integer-digit-separators",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 1_000_000 AS separated_integer",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("digit-separated integer RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("1000000".to_owned())]]
    );
}

#[test]
fn should_truncate_identifiers_at_the_utf8_safe_postgres_byte_limit() {
    // Arrange
    let at_limit = "a".repeat(62);
    let before_multibyte = "a".repeat(61);
    let multibyte_within_limit = format!("{before_multibyte}é");
    let sql = format!("SELECT 1 AS \"{at_limit}é\", 2 AS \"{multibyte_within_limit}\"");
    let batches = run_script(
        "issue-796-unicode-identifier-truncation",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(&sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("identifier truncation RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![at_limit.as_str(), multibyte_within_limit.as_str()]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("1".to_owned()), Some("2".to_owned())]]
    );
}

#[test]
fn should_report_postgres_collation_conflicts_with_sqlstate_42p21() {
    // Arrange
    let batches = run_script(
        "issue-796-collation-conflict-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ('a' COLLATE \"C\") < ('b' COLLATE \"POSIX\")",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42P21"));
}

#[test]
fn should_expose_the_builtin_postgres_collation_catalog_rows() {
    // Arrange
    let batches = run_script(
        "issue-796-pg-collation-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT collname, collprovider, collisdeterministic FROM pg_collation WHERE collname = 'C'",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        wire::row_description_names(result),
        vec!["collname", "collprovider", "collisdeterministic"]
    );
    assert_eq!(wire::data_rows(result).len(), 1);
    assert_eq!(wire::data_rows(result)[0][0].as_deref(), Some("C"));
}

#[test]
fn should_preserve_nondeterministic_collation_for_equality_grouping_unique_indexes() {
    // Arrange
    let batches = run_script(
        "issue-796-nondeterministic-collation-index-grouping",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE COLLATION issue_796_ci_index (provider = icu, locale = 'und-u-ks-level2', deterministic = false)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_796_ci_values (value TEXT COLLATE issue_796_ci_index)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_796_ci_values VALUES ('A'), ('a'), ('b')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE INDEX issue_796_ci_values_value_idx ON issue_796_ci_values (value)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT count(*) FROM issue_796_ci_values WHERE value = 'a'",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT count(*) FROM issue_796_ci_values GROUP BY value ORDER BY count(*) DESC",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_796_unique_ci_values (value TEXT COLLATE issue_796_ci_index)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE UNIQUE INDEX issue_796_unique_ci_values_value_idx ON issue_796_unique_ci_values (value)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_796_unique_ci_values VALUES ('A')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_796_unique_ci_values VALUES ('a')",
            )]),
        ],
    );

    // Act
    let setup = &batches[..4];
    let indexed_equality = &batches[4];
    let grouped = &batches[5];
    let unique_index_setup = &batches[6..9];
    let duplicate = &batches[9];

    // Assert
    let setup_errors = setup
        .iter()
        .flat_map(|frames| {
            frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
        })
        .collect::<Vec<_>>();
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "collation and index setup errors: {setup_errors:?}"
    );
    assert_eq!(wire::error_code(indexed_equality), None);
    assert_eq!(
        wire::data_rows(indexed_equality),
        vec![vec![Some("2".into())]]
    );
    assert_eq!(wire::error_code(grouped), None);
    assert_eq!(
        wire::data_rows(grouped),
        vec![vec![Some("2".into())], vec![Some("1".into())]]
    );
    assert!(
        unique_index_setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "unique collation index setup errors: {:?}",
        unique_index_setup
            .iter()
            .flat_map(|frames| {
                frames
                    .iter()
                    .filter(|frame| frame.0 == b'E')
                    .map(|frame| wire::parse_error_fields(&frame.1))
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::error_code(duplicate).as_deref(), Some("23505"));
}

#[test]
fn should_describe_parenthesized_parameter_arithmetic_in_offset() {
    // Arrange
    let batches = run_script(
        "issue-800-parenthesized-offset-arithmetic",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "good_offset",
                "SELECT 1 OFFSET ($1 + 1) ROWS",
                &[23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("good_offset"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![23]);
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 23);
}

#[test]
fn should_reject_unparenthesized_parameter_arithmetic_in_offset_with_syntax_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-800-unparenthesized-offset-arithmetic",
        vec![Step::ParseFlush(wire::parse_frame(
            "bad_offset",
            "SELECT 1 OFFSET $1 + 1 ROWS",
        ))],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42601"));
    let error = result
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("unparenthesized OFFSET Parse error");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, value)| value.as_str()),
        Some("syntax error at or near \"ROWS\"")
    );
}

#[test]
fn should_report_malformed_unicode_identifier_escape_with_syntax_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-796-malformed-unicode-identifier",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT U&\"bad!00G0\" UESCAPE '!'",
        )])],
    );

    // Act
    let result = &batches[0];
    let error = result
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("malformed Unicode identifier ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42601"));
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, value)| value.as_str()),
        Some("invalid Unicode escape")
    );
}

#[test]
fn should_report_unsupported_comparison_for_empty_rows_with_0a000() {
    // Arrange
    let batches = run_script(
        "issue-800-empty-row-equality-sqlstate",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ROW() = ROW()",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("0A000"));
}

#[test]
fn should_compare_values_from_literal_row_constructors() {
    // Arrange
    let batches = run_script(
        "issue-800-literal-row-equality",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ROW(1, 2) = ROW(1, 2)",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_report_undefined_operator_for_literal_row_image_comparisons() {
    // Arrange
    let operators = ["*=", "*<>", "*<", "*<=", "*>", "*>="];
    let steps = operators
        .iter()
        .map(|operator| {
            Step::ThroughReady(vec![wire::simple_query_frame(&format!(
                "SELECT ROW(1, 2) {operator} ROW(1, 2)"
            ))])
        })
        .collect();
    let batches = run_script("issue-800-row-image-operator-sqlstate", steps);

    // Act
    let errors = batches
        .iter()
        .map(|frames| wire::error_code(frames))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(errors, vec![Some("42883".to_owned()); operators.len()]);
}

#[test]
fn should_sample_rows_with_a_valid_bernoulli_percentage() {
    // Arrange
    let batches = run_script(
        "issue-800-tablesample-valid-rate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_sample_valid_rows (id INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_800_sample_valid_rows VALUES (1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_valid_rows TABLESAMPLE BERNOULLI (100)",
            )]),
        ],
    );

    // Act
    let created = &batches[0];
    let inserted = &batches[1];
    let sampled = &batches[2];

    // Assert
    assert_eq!(wire::error_code(created), None);
    assert_eq!(wire::error_code(inserted), None);
    assert_eq!(wire::error_code(sampled), None);
    assert_eq!(wire::data_rows(sampled), vec![vec![Some("1".to_owned())]]);
}

#[test]
fn should_bind_parameterized_bernoulli_sampling_over_extended_protocol() {
    // Arrange
    let rows = (1..=128)
        .map(|id| format!("({id})"))
        .collect::<Vec<_>>()
        .join(", ");
    let insert = format!("INSERT INTO issue_800_sample_wire_rows VALUES {rows}");
    let sql = "SELECT id FROM issue_800_sample_wire_rows TABLESAMPLE BERNOULLI ($1) REPEATABLE ($2) ORDER BY id";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_800_sample_wire_rows (id INT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(&insert)]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "sample_parameters",
            sql,
            &[700, 701],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("sample_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("sample_portal", "sample_parameters", &["100", "0.5"]),
            wire::describe_portal_frame("sample_portal"),
            wire::execute_frame("sample_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-800-tablesample-parameter-wire", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        batches[2].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1'],
        "TABLESAMPLE Parse error fields: {:?}",
        batches[2]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::error_code(&batches[3]), None);
    let parameters = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("TABLESAMPLE ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![700, 701]
    );
    let columns = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("TABLESAMPLE result RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 23);
    assert_eq!(wire::error_code(&batches[4]), None);
    assert_eq!(
        wire::data_rows(&batches[4]),
        (1..=128)
            .map(|id| vec![Some(id.to_string())])
            .collect::<Vec<_>>()
    );
}

#[test]
fn should_cancel_tablesample_filter_without_partial_rows_or_stale_state() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-800-tablesample-cancel");
    let _directory = GrammarFixtureDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let (setup, baseline, cancelled, recovery) = runtime.block_on(async {
        let cassie = cassie::app::Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie
            .startup()
            .expect("start TABLESAMPLE cancellation fixture");
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = socket.into_split();
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_sample_cancel_rows (id INT); \
                 INSERT INTO issue_800_sample_cancel_rows VALUES (1), (2), (3)",
            )],
        )
        .await;
        let setup = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_cancel_rows \
                 TABLESAMPLE BERNOULLI (100) \
                 WHERE pg_sleep_for(INTERVAL '0 seconds') IS NOT NULL ORDER BY id",
            )],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_cancel_rows \
                 TABLESAMPLE BERNOULLI (100) \
                 WHERE pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL ORDER BY id",
            )],
        )
        .await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        let mut cancel_socket = tokio::net::TcpStream::connect(server.addr)
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
        server.stop().await;
        (setup, baseline, cancelled, recovery)
    });

    // Assert
    assert_eq!(wire::error_code(&setup), None);
    assert_eq!(wire::error_code(&baseline), None);
    assert_eq!(
        wire::data_rows(&baseline),
        vec![
            vec![Some("1".to_owned())],
            vec![Some("2".to_owned())],
            vec![Some("3".to_owned())],
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
fn should_describe_values_relation_columns_over_pgwire() {
    // Arrange
    let batches = run_script(
        "issue-800-values-row-wire-metadata",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT * FROM (VALUES (1::BIGINT, 'alpha'::TEXT), (NULL::BIGINT, 'beta'::TEXT)) AS v(id, label)",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    let error_fields = result
        .iter()
        .find(|frame| frame.0 == b'E')
        .map(|frame| wire::parse_error_fields(&frame.1));
    assert_eq!(
        wire::error_code(result),
        None,
        "valid VALUES relation failed: {error_fields:?}"
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("typed VALUES RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "label"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [20, 25]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("1".to_owned()), Some("alpha".to_owned())],
            vec![None, Some("beta".to_owned())],
        ]
    );
}

#[test]
fn should_bind_oid_zero_bigint_parameters_in_values_over_extended_protocol() {
    // Arrange
    let sql =
        "SELECT v.id FROM (VALUES ($1::BIGINT), ($2::BIGINT)) AS v(id) ORDER BY id NULLS FIRST";
    let high_value = 9_007_199_254_740_993_i64.to_be_bytes();
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "values_bigint_parameters",
            sql,
            &[0, 0],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("values_bigint_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "values_bigint_portal",
                "values_bigint_parameters",
                &[1],
                &[Some(&high_value), None],
                &[1],
            ),
            wire::describe_portal_frame("values_bigint_portal"),
            wire::execute_frame("values_bigint_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-800-values-parameter-wire", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "VALUES parameter Parse SQLSTATE; frames: {:?}",
        batches[0]
    );
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    let parameters = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("inferred VALUES ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![20, 20]
    );
    let statement_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("VALUES statement RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    assert_eq!(statement_columns[0].name, "id");
    assert_eq!(statement_columns[0].type_oid, 20);

    let bound = &batches[2];
    assert_eq!(wire::error_code(bound), None);
    let portal_description = bound
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary VALUES portal RowDescription");
    let portal_columns = wire::parse_row_description(&portal_description.1);
    assert_eq!(portal_columns[0].type_oid, 20);
    assert_eq!(portal_columns[0].format_code, 1);
    let data_rows = bound
        .iter()
        .filter(|(tag, _)| *tag == b'D')
        .map(|(_, payload)| payload.as_slice())
        .collect::<Vec<_>>();
    assert_eq!(
        data_rows,
        vec![
            &[0, 1, 255, 255, 255, 255][..],
            &[0, 1, 0, 0, 0, 8, 0, 32, 0, 0, 0, 0, 0, 1][..],
        ]
    );
}

#[test]
fn should_report_invalid_table_sample_percentages_with_2202h() {
    // Arrange
    let batches = run_script(
        "issue-800-tablesample-range-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_sample_error_rows (id INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_800_sample_error_rows VALUES (1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_error_rows TABLESAMPLE BERNOULLI (-1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_error_rows TABLESAMPLE SYSTEM (101)",
            )]),
        ],
    );

    // Act
    let created = &batches[0];
    let inserted = &batches[1];
    let negative_rate = &batches[2];
    let overfull_rate = &batches[3];

    // Assert
    assert_eq!(wire::error_code(created), None);
    assert_eq!(wire::error_code(inserted), None);
    assert_eq!(wire::error_code(negative_rate).as_deref(), Some("2202H"));
    assert_eq!(wire::error_code(overfull_rate).as_deref(), Some("2202H"));
}

#[test]
fn should_report_values_using_row_arity_sqlstates() {
    // Arrange
    let cases = [
        ("VALUES (1, 2), (3)", "42601"),
        ("SELECT * FROM (VALUES (1, 2)) AS v(x, y, z)", "42P10"),
        ("SELECT * FROM (VALUES (1), (TRUE)) AS v(value)", "42804"),
        ("SELECT ROW(1, 2) = ROW(1)", "42601"),
        (
            "SELECT a.id FROM (VALUES (1)) AS a(id) JOIN (VALUES (1)) AS b(id) USING (missing)",
            "42703",
        ),
        (
            "SELECT * FROM (VALUES (1)) AS a(id) JOIN (VALUES (1)) AS b(id) USING (id, id)",
            "42701",
        ),
    ];
    let steps = cases
        .iter()
        .map(|(sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("issue-800-values-using-row-errors", steps);
    let actual = batches
        .iter()
        .map(|batch| wire::error_code(batch))
        .collect::<Vec<_>>();
    let expected = cases
        .iter()
        .map(|(_, state)| Some((*state).to_owned()))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(actual, expected);
}

#[test]
fn should_return_ordered_integer_rows_from_generate_series() {
    // Arrange
    let batches = run_script(
        "issue-808-generate-series-integer-rows",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT generate_series(1, 3)",
        )])],
    );

    // Act
    let frames = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire error fields: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("generate_series RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned())],
            vec![Some("2".to_owned())],
            vec![Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_return_lockstep_columns_from_rows_from_sources() {
    // Arrange
    let batches = run_script(
        "issue-808-rows-from-lockstep-sources",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT * FROM ROWS FROM(generate_series(1, 3), generate_series(10, 12)) AS generated(first_value, second_value)",
        )])],
    );

    // Act
    let frames = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire error fields: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("ROWS FROM RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("11".to_owned())],
            vec![Some("3".to_owned()), Some("12".to_owned())],
        ]
    );
}
