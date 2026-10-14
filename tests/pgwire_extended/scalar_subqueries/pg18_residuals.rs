//! Residual PostgreSQL 18 scalar and row-subquery contracts for #762.

use std::sync::Arc;
use std::time::Duration;

use super::setup;
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture_with_config;
use cassie::config::CassieRuntimeConfig;

#[test]
fn should_aggregate_correlated_scalar_subquery_results() {
    // Arrange
    let sql = "SELECT SUM((SELECT inner_row.value FROM scalar_wire_values AS inner_row \
                   WHERE inner_row.id = outer_row.id)) AS total \
               FROM scalar_wire_values AS outer_row";

    // Act
    let batches = run_script(
        "issue-762-correlated-scalar-subquery-in-aggregate",
        vec![
            setup(),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    );
    let result = &batches[1];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "correlated scalar-subquery aggregate ErrorResponse: {:?}",
        result
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("aggregate scalar-subquery RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 20);
    assert_eq!(wire::data_rows(result), vec![vec![Some("300".to_owned())]]);
}

#[test]
fn should_match_quantified_comparison_operator_truth_tables() {
    // Arrange
    let operators = ["=", "<>", "<", "<=", ">", ">="];
    let scenarios = [
        (
            "non_null",
            "SELECT n FROM (VALUES (1::INT4), (3::INT4)) AS candidates(n)",
            [
                Some("f"),
                Some("t"),
                Some("t"),
                Some("t"),
                Some("t"),
                Some("t"),
            ],
            [
                Some("f"),
                Some("t"),
                Some("f"),
                Some("f"),
                Some("f"),
                Some("f"),
            ],
        ),
        (
            "with_null",
            "SELECT n FROM (VALUES (1::INT4), (3::INT4), (NULL::INT4)) AS candidates(n)",
            [None, Some("t"), Some("t"), Some("t"), Some("t"), Some("t")],
            [Some("f"), None, Some("f"), Some("f"), Some("f"), Some("f")],
        ),
        (
            "empty",
            "SELECT 1::INT4 AS n WHERE false",
            [Some("f"); 6],
            [Some("t"); 6],
        ),
    ];
    let mut cases = Vec::new();
    let mut steps = Vec::new();
    for (scenario, candidates, any_results, all_results) in scenarios {
        for (quantifier, results) in [("ANY", any_results), ("ALL", all_results)] {
            for (index, operator) in operators.iter().enumerate() {
                let sql =
                    format!("SELECT 2 {operator} {quantifier} ({candidates}) AS comparison_result");
                cases.push((scenario, quantifier, *operator, sql, results[index]));
                steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                    cases.last().expect("just-added comparison case").3.as_str(),
                )]));
            }
        }
    }

    // Act
    let batches = run_script("issue-762-any-all-operator-truth-tables", steps);

    // Assert
    assert_eq!(batches.len(), cases.len());
    for ((scenario, quantifier, operator, sql, expected), result) in cases.iter().zip(&batches) {
        assert_eq!(
            wire::error_code(result),
            None,
            "unexpected error for {scenario} {quantifier} {operator}: {sql}"
        );
        let description = result
            .iter()
            .find(|frame| frame.0 == b'T')
            .unwrap_or_else(|| panic!("missing Boolean RowDescription for {sql}"));
        assert_eq!(
            wire::parse_row_description(&description.1)[0].type_oid,
            16,
            "comparison result type for {sql}"
        );
        assert_eq!(
            wire::data_rows(result),
            vec![vec![expected.map(str::to_owned)]],
            "PostgreSQL 18.6 truth value for {sql}"
        );
    }
}

#[test]
fn should_preserve_correlated_row_subquery_null_truth_values() {
    // Arrange
    let sql = "WITH outer_values(id, a, b) AS (\
                   VALUES (1::INT4, 1::INT4, 10::INT4),\
                          (2::INT4, 2::INT4, 20::INT4),\
                          (3::INT4, 3::INT4, 30::INT4),\
                          (4::INT4, 4::INT4, 40::INT4)\
               ), inner_values(parent_id, x, y) AS (\
                   VALUES (1::INT4, 1::INT4, 10::INT4),\
                          (2::INT4, 2::INT4, NULL::INT4),\
                          (4::INT4, 4::INT4, 41::INT4)\
               )\
               SELECT id,\
                      ROW(a, b) = (SELECT x, y FROM inner_values i\
                                   WHERE i.parent_id = o.id) AS row_equal,\
                      ROW(a, b) IN (SELECT x, y FROM inner_values i\
                                    WHERE i.parent_id = o.id) AS row_in,\
                      ROW(a, b) NOT IN (SELECT x, y FROM inner_values i\
                                        WHERE i.parent_id = o.id) AS row_not_in\
               FROM outer_values o ORDER BY id";
    let batches = run_script(
        "issue-762-correlated-row-subquery-truth-table",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("correlated row-subquery RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 16, 16, 16]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "row_equal", "row_in", "row_not_in"]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("1".to_owned()),
                Some("t".to_owned()),
                Some("t".to_owned()),
                Some("f".to_owned())
            ],
            vec![Some("2".to_owned()), None, None, None],
            vec![
                Some("3".to_owned()),
                None,
                Some("f".to_owned()),
                Some("t".to_owned())
            ],
            vec![
                Some("4".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned()),
                Some("t".to_owned())
            ],
        ]
    );
}

#[test]
fn should_execute_binary_row_subquery_with_described_metadata() {
    // Arrange
    let sql = "SELECT ROW($1::INT4, $2::INT4) = (\
                   SELECT candidate.x, candidate.y\
                   FROM (VALUES (1::INT4, 10::INT4)) AS candidate(x, y)\
               ) AS matches";
    let first_value = 1_i32.to_be_bytes();
    let second_value = 10_i32.to_be_bytes();
    let batches = run_script(
        "issue-762-row-subquery-binary-parameters",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types("row_subquery", sql, &[0, 0])),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("row_subquery"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "row_subquery_binary",
                    "row_subquery",
                    &[1],
                    &[Some(&first_value), Some(&second_value)],
                    &[1],
                ),
                wire::describe_portal_frame("row_subquery_binary"),
                wire::execute_frame("row_subquery_binary"),
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
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("row-subquery ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![23, 23]
    );
    let statement_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-subquery statement RowDescription");
    let statement_columns = wire::parse_row_description(&statement_description.1);
    assert_eq!(statement_columns[0].name, "matches");
    assert_eq!(statement_columns[0].type_oid, 16);

    assert_eq!(wire::error_code(executed), None);
    let portal_description = executed
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary row-subquery portal RowDescription");
    let portal_column = &wire::parse_row_description(&portal_description.1)[0];
    assert_eq!(portal_column.type_oid, 16);
    assert_eq!(portal_column.format_code, 1);
    let data_row = executed
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary row-subquery DataRow");
    assert_eq!(
        i16::from_be_bytes(data_row.1[0..2].try_into().expect("field count")),
        1
    );
    assert_eq!(
        i32::from_be_bytes(data_row.1[2..6].try_into().expect("field length")),
        1
    );
    assert_eq!(data_row.1[6], 1);
}

#[test]
fn should_apply_result_row_limit_atomically_to_scalar_subquery_projection() {
    // Arrange
    wire::use_local_storage();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 2;
    let storage = sql_fixture_with_config("issue-762-scalar-result-limit", &[], config.clone());
    storage
        .cassie
        .startup()
        .expect("start scalar result-limit fixture");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let (setup, within_limit, overflow, recovery) = runtime.block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            Arc::new(storage.cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_cap_outer (id INT4 PRIMARY KEY);\
                 CREATE TABLE issue_762_cap_inner (id INT4, value INT4);\
                 INSERT INTO issue_762_cap_outer VALUES (1), (2), (3);\
                 INSERT INTO issue_762_cap_inner VALUES (1, 10), (2, 20), (3, 30)",
            )],
        )
        .await;
        let setup = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT o.id, (SELECT i.value FROM issue_762_cap_inner i WHERE i.id = o.id) AS scalar_value\
                 FROM issue_762_cap_outer o WHERE o.id <= 2 ORDER BY o.id",
            )],
        )
        .await;
        let within_limit =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT o.id, (SELECT i.value FROM issue_762_cap_inner i WHERE i.id = o.id) AS scalar_value\
                 FROM issue_762_cap_outer o ORDER BY o.id",
            )],
        )
        .await;
        let overflow =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 46 AS recovery")])
            .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (setup, within_limit, overflow, recovery)
    });
    drop(runtime);

    // Assert
    assert_eq!(wire::error_code(&setup), None);
    assert_eq!(wire::error_code(&within_limit), None);
    assert_eq!(
        wire::data_rows(&within_limit),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
        ]
    );
    assert_eq!(wire::error_code(&overflow).as_deref(), Some("54000"));
    assert!(wire::data_rows(&overflow).is_empty());
    assert_eq!(overflow.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(overflow.last().map(|frame| frame.0), Some(b'Z'));
    let error = overflow
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("scalar result-limit ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M' && message == "query result row limit exceeded: 3 > 2"
    }));
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("46".to_owned())]]
    );
}

#[test]
fn should_accept_scalar_subquery_in_case_expression() {
    // Arrange
    let batches = run_script(
        "issue-762-case-scalar-subquery-short-circuit",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_unselected_scalar_error (denominator INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_unselected_scalar_error VALUES (0)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT CASE WHEN FALSE THEN \
                    (SELECT 10 / (denominator - denominator) \
                     FROM issue_762_unselected_scalar_error) \
                 ELSE 7 END AS case_value",
            )]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let case_result = &batches[2];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "scalar-subquery setup errors: {:?}",
        setup
            .iter()
            .flat_map(|frames| frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(case_result),
        None,
        "CASE scalar-subquery error: {:?}",
        case_result
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let case_description = case_result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("CASE scalar-subquery RowDescription");
    assert_eq!(
        wire::parse_row_description(&case_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23]
    );
    assert_eq!(wire::data_rows(case_result), vec![vec![Some("7".into())]]);
}

#[test]
fn should_accept_scalar_subquery_in_coalesce_expression() {
    // Arrange
    let batches = run_script(
        "issue-762-coalesce-scalar-subquery-short-circuit",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_unselected_coalesce_error (denominator INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_unselected_coalesce_error VALUES (0)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT COALESCE(9, \
                    (SELECT 10 / (denominator - denominator) \
                     FROM issue_762_unselected_coalesce_error)) AS coalesce_value",
            )]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let coalesce_result = &batches[2];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "scalar-subquery setup errors: {:?}",
        setup
            .iter()
            .flat_map(|frames| frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(coalesce_result),
        None,
        "COALESCE scalar-subquery error: {:?}",
        coalesce_result
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let coalesce_description = coalesce_result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("COALESCE scalar-subquery RowDescription");
    assert_eq!(
        wire::parse_row_description(&coalesce_description.1)[0].type_oid,
        23
    );
    assert_eq!(
        wire::data_rows(coalesce_result),
        vec![vec![Some("9".into())]]
    );
}
