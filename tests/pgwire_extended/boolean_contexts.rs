//! Reuses the established wire fixture and caller-specific SQLSTATE families.

use super::support_pgwire as support;
use cassie::app::Cassie;
use cassie::types::DataType;

type WireFrame = (u8, Vec<u8>);

#[test]
fn should_decode_canonical_boolean_elements_in_declared_text_array_bind() {
    // Arrange
    // Preserve Cassie's current synthetic ARRAY OID and JSON text output.
    // PostgreSQL ARRAY OID/output parity belongs to the separate #752 profile.
    let oid = i32::try_from(DataType::Array(Box::new(DataType::Boolean)).type_oid())
        .expect("current declared ARRAY OID");
    let cases = [
        (
            "SELECT $1 AS flags",
            "{tr,of,NULL,\"  Ye  \"}",
            Some("[true,false,null,true]"),
        ),
        ("SELECT $1 AS flags", "{}", Some("[]")),
        ("SELECT $1 AS flags LIMIT 0", "{tr,of,NULL}", None),
    ];
    let cycles = cases
        .map(|(sql, input, _)| text_cycle(sql, oid, input))
        .to_vec();

    // Act
    let batches = run_cycles("bool-wire-array-vocabulary", cycles);

    // Assert
    for ((sql, _, expected), frames) in cases.into_iter().zip(batches) {
        assert_eq!(support::error_code(&frames), None, "{sql}");
        assert_eq!(parameter_oids(&frames), vec![oid]);
        assert_eq!(result_oids(&frames), vec![oid]);
        let expected_rows =
            expected.map_or_else(Vec::new, |value| vec![vec![Some(value.to_string())]]);
        assert_eq!(support::data_rows(&frames), expected_rows, "{sql}");
    }
}

#[test]
fn should_preserve_declared_boolean_array_bind_rejection() {
    // Arrange
    let oid = i32::try_from(DataType::Array(Box::new(DataType::Boolean)).type_oid())
        .expect("current declared ARRAY OID");
    // Unquoted NULL remains an array NULL; quoted NULL remains invalid Bool input.
    let inputs = ["{o}", "{maybe}", "{\"NULL\"}"];
    let cycles = inputs
        .map(|input| text_cycle("SELECT $1 AS flags LIMIT 0", oid, input))
        .to_vec();

    // Act
    let batches = run_cycles("bool-wire-array-invalid", cycles);

    // Assert
    for (input, frames) in inputs.into_iter().zip(batches) {
        assert_eq!(
            support::error_code(&frames).as_deref(),
            Some("08P01"),
            "{input}"
        );
        assert_eq!(
            support::data_rows(&frames),
            Vec::<Vec<Option<String>>>::new()
        );
    }
}

fn run_cycles(label: &str, cycles: Vec<Vec<Vec<u8>>>) -> Vec<Vec<WireFrame>> {
    support::use_local_storage();
    let path = support::data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let batches = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE bool_wire (id INT PRIMARY KEY, n INT, flag BOOLEAN)",
            "INSERT INTO bool_wire (id, n, flag) VALUES (1, 10, TRUE)",
            "CREATE TABLE bool_wire_empty (id INT, n INT, flag BOOLEAN)",
        ] {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed wire fixture");
        }
        let server = support::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (mut reader, mut writer) = tokio::io::split(socket);
        support::complete_startup(&mut reader, &mut writer).await;
        let mut batches = Vec::new();
        for cycle in cycles {
            support::write_frames(&mut writer, cycle).await;
            batches.push(
                support::read_frames_until_ready_within(
                    &mut reader,
                    std::time::Duration::from_secs(5),
                )
                .await,
            );
        }
        server.stop().await;
        batches
    });
    let _ = std::fs::remove_dir_all(path);
    batches
}

fn text_cycle(sql: &str, oid: i32, value: &str) -> Vec<Vec<u8>> {
    vec![
        support::parse_frame_with_types("", sql, &[oid]),
        support::describe_statement_frame(""),
        support::bind_frame("", "", &[value]),
        support::execute_frame(""),
        support::sync_frame(),
    ]
}

fn parameter_oids(frames: &[WireFrame]) -> Vec<i32> {
    let (_, payload) = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    support::parse_parameter_description(payload)
}

fn result_oids(frames: &[WireFrame]) -> Vec<i32> {
    let (_, payload) = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    support::parse_row_description(payload)
        .into_iter()
        .map(|column| column.type_oid)
        .collect()
}

#[test]
fn should_infer_boolean_parameter_oids_at_each_predicate_sink() {
    // Arrange
    let statements = [
        "SELECT 1 WHERE $1",
        "SELECT $1 AND TRUE",
        "SELECT FALSE OR $1",
        "SELECT NOT $1",
        "SELECT CASE WHEN $1 THEN 1 ELSE 2 END",
        "SELECT bool_wire.id FROM bool_wire JOIN bool_wire_empty ON $1",
        "SELECT COUNT(*) FROM bool_wire HAVING $1",
        "UPDATE bool_wire SET n = 99 WHERE $1 RETURNING n",
        "DELETE FROM bool_wire WHERE $1 RETURNING n",
        "INSERT INTO bool_wire (id, n, flag) VALUES (1, 99, TRUE) ON CONFLICT (id) DO UPDATE SET n = excluded.n WHERE $1 RETURNING n",
    ];
    let cases = [0, 705]
        .into_iter()
        .flat_map(|oid| statements.map(|sql| (sql, oid)))
        .collect::<Vec<_>>();
    let cycles = cases
        .iter()
        .map(|(sql, oid)| text_cycle(sql, *oid, "no"))
        .collect();

    // Act
    let batches = run_cycles("bool-wire-context-oids", cycles);

    // Assert
    for ((sql, supplied_oid), frames) in cases.iter().zip(batches) {
        assert_eq!(
            support::error_code(&frames),
            None,
            "{sql}, OID {supplied_oid}"
        );
        assert_eq!(
            parameter_oids(&frames),
            vec![16],
            "{sql}, OID {supplied_oid}"
        );
    }
}

#[test]
fn should_describe_contextual_boolean_metadata_before_row_evaluation() {
    // Arrange
    let sql = "SELECT $1 AS predicate FROM bool_wire_empty WHERE $1";
    let cycles = [0, 705].map(|oid| text_cycle(sql, oid, "no")).to_vec();

    // Act
    let batches = run_cycles("bool-wire-empty-metadata", cycles);

    // Assert
    for frames in batches {
        assert_eq!(support::error_code(&frames), None);
        assert_eq!(parameter_oids(&frames), vec![16]);
        assert_eq!(result_oids(&frames), vec![16]);
        assert_eq!(
            support::data_rows(&frames),
            Vec::<Vec<Option<String>>>::new()
        );
    }
}

#[test]
fn should_infer_boolean_parameters_through_supported_query_scopes() {
    // Arrange
    let statements = [
        "WITH c AS (SELECT n FROM bool_wire_empty WHERE $1) SELECT n FROM c",
        "SELECT n FROM (SELECT n FROM bool_wire_empty WHERE NOT $1) AS derived",
        "SELECT n FROM bool_wire_empty WHERE EXISTS (SELECT n FROM bool_wire WHERE $1 AND TRUE)",
        "SELECT bool_wire.id FROM bool_wire JOIN (SELECT n FROM bool_wire_empty) AS derived ON $1",
    ];
    let cycles = statements.map(|sql| text_cycle(sql, 0, "no")).to_vec();

    // Act
    let batches = run_cycles("bool-wire-nested-oids", cycles);

    // Assert
    for (sql, frames) in statements.into_iter().zip(batches) {
        assert_eq!(support::error_code(&frames), None, "{sql}");
        assert_eq!(parameter_oids(&frames), vec![16], "{sql}");
        assert_eq!(
            support::data_rows(&frames),
            Vec::<Vec<Option<String>>>::new(),
            "{sql}"
        );
    }
}

#[test]
fn should_reject_declared_non_boolean_parameters_in_boolean_contexts() {
    // Arrange
    let cases = [
        ("SELECT 1 WHERE $1", 23, "1"),
        ("SELECT 1 WHERE $1", 25, "no"),
        ("SELECT NOT $1", 25, "false"),
        ("SELECT $1 AND FALSE", 23, "1"),
        ("SELECT CASE WHEN $1 THEN 1 ELSE 2 END", 25, "no"),
        ("SELECT n FROM bool_wire_empty WHERE $1", 25, "true"),
    ];
    let cycles = cases
        .map(|(sql, oid, value)| text_cycle(sql, oid, value))
        .to_vec();

    // Act
    let batches = run_cycles("bool-wire-declared-types", cycles);

    // Assert
    for ((sql, oid, _), frames) in cases.into_iter().zip(batches) {
        assert_eq!(
            support::error_code(&frames).as_deref(),
            Some("42601"),
            "{sql}, OID {oid}"
        );
        assert_eq!(
            support::data_rows(&frames),
            Vec::<Vec<Option<String>>>::new()
        );
    }
}

#[test]
fn should_reject_incompatible_repeated_parameter_contexts_independent_of_traversal_order() {
    // Arrange
    let statements = [
        "SELECT 1 WHERE ($1 AND TRUE) AND ($1 = 1)",
        "SELECT 1 WHERE ($1 = 1) AND ($1 AND TRUE)",
        "SELECT n FROM bool_wire_empty WHERE ($1 AND TRUE) AND ($1 = n)",
        "SELECT n FROM bool_wire_empty WHERE ($1 = n) AND ($1 AND TRUE)",
    ];
    let cycles = statements.map(|sql| text_cycle(sql, 0, "1")).to_vec();

    // Act
    let batches = run_cycles("bool-wire-repeated-contexts", cycles);

    // Assert
    for (sql, frames) in statements.into_iter().zip(batches) {
        assert_eq!(
            support::error_code(&frames).as_deref(),
            Some("42601"),
            "{sql}"
        );
        assert_eq!(
            support::data_rows(&frames),
            Vec::<Vec<Option<String>>>::new()
        );
    }
}

#[test]
fn should_decode_contextual_boolean_values_with_boolean_metadata() {
    // Arrange
    let sql = "SELECT NOT $1 AS flag";
    let binary_false = [0_u8];
    let binary_true = [1_u8];
    let cases = [
        (0, 0, Some(b"no".as_slice()), Some("t")),
        (705, 0, Some(b"of".as_slice()), Some("t")),
        (0, 0, Some(b"  tr  ".as_slice()), Some("f")),
        (16, 0, Some(b"yes".as_slice()), Some("f")),
        (0, 1, Some(binary_false.as_slice()), Some("t")),
        (705, 1, Some(binary_true.as_slice()), Some("f")),
        (0, 0, None, None),
    ];
    let cycles = cases
        .iter()
        .map(|(oid, format, input, _)| {
            vec![
                support::parse_frame_with_types("", sql, &[*oid]),
                support::describe_statement_frame(""),
                support::bind_frame_with_formats("", "", &[*format], &[*input], &[]),
                support::execute_frame(""),
                support::sync_frame(),
            ]
        })
        .collect();

    // Act
    let batches = run_cycles("bool-wire-codec-metadata", cycles);

    // Assert
    for ((oid, format, _, expected), frames) in cases.into_iter().zip(batches) {
        assert_eq!(
            support::error_code(&frames),
            None,
            "OID {oid}, format {format}"
        );
        assert_eq!(parameter_oids(&frames), vec![16]);
        assert_eq!(result_oids(&frames), vec![16]);
        assert_eq!(
            support::data_rows(&frames),
            vec![vec![expected.map(str::to_string)]]
        );
    }
}

#[test]
fn should_preserve_existing_boolean_adapter_error_families() {
    // Arrange
    let cycles = vec![
        vec![support::simple_query_frame("SELECT 1 WHERE 1")],
        vec![support::simple_query_frame("SELECT 1 WHERE 'o'")],
        vec![support::simple_query_frame("SELECT CAST('o' AS BOOLEAN)")],
        text_cycle("SELECT NOT $1", 16, "o"),
    ];

    // Act
    let batches = run_cycles("bool-wire-existing-error-families", cycles);

    // Assert
    for (frames, expected) in batches.iter().zip(["42601", "42601", "22000", "08P01"]) {
        assert_eq!(support::error_code(frames).as_deref(), Some(expected));
        assert_eq!(
            support::data_rows(frames),
            Vec::<Vec<Option<String>>>::new()
        );
    }
}

#[test]
fn should_preserve_explicit_numeric_parameter_casts_to_boolean() {
    // Arrange
    let cases = [
        ("SELECT CAST($1 AS BOOLEAN) AS flag", "0", Some("f")),
        ("SELECT CAST($1 AS BOOLEAN) AS flag", "1", Some("t")),
        ("SELECT CAST($1 AS BOOLEAN) AS flag", "-2", Some("t")),
        (
            "SELECT 1 FROM bool_wire_empty WHERE CAST($1 AS BOOLEAN)",
            "1",
            None,
        ),
    ];
    let cycles = cases
        .map(|(sql, input, _)| text_cycle(sql, 23, input))
        .to_vec();

    // Act
    let batches = run_cycles("bool-wire-explicit-numeric-cast", cycles);

    // Assert
    for ((sql, _, expected), frames) in cases.into_iter().zip(batches) {
        assert_eq!(support::error_code(&frames), None, "{sql}");
        assert_eq!(
            parameter_oids(&frames),
            vec![23],
            "declared INT remains INT"
        );
        let expected_oid = if expected.is_some() { 16 } else { 23 };
        assert_eq!(result_oids(&frames), vec![expected_oid], "{sql}");
        let expected_rows =
            expected.map_or_else(Vec::new, |text| vec![vec![Some(text.to_string())]]);
        assert_eq!(support::data_rows(&frames), expected_rows, "{sql}");
    }
}

#[test]
fn should_preserve_wire_boolean_comparison_parameter_provenance() {
    // Arrange
    let accepted = [
        ("SELECT $1 = 'no' AS matched", Some("t")),
        ("SELECT 'no' = $1 AS matched", Some("t")),
        ("SELECT $1 = 'no' AS matched FROM bool_wire_empty", None),
    ];
    let rejected = [
        "SELECT $1 = CAST('no' AS TEXT) AS matched",
        "SELECT $1 = 1 AS matched",
        "SELECT $1 = CAST('no' AS TEXT) AS matched FROM bool_wire_empty",
        "SELECT $1 = 1 AS matched FROM bool_wire_empty",
    ];
    let cycles = accepted
        .iter()
        .map(|(sql, _)| *sql)
        .chain(rejected)
        .map(|sql| text_cycle(sql, 16, "no"))
        .collect();

    // Act
    let batches = run_cycles("bool-wire-comparison-parameter-types", cycles);
    let (accepted_frames, rejected_frames) = batches.split_at(accepted.len());

    // Assert
    for ((sql, expected), frames) in accepted.into_iter().zip(accepted_frames) {
        assert_eq!(support::error_code(frames), None, "{sql}");
        assert_eq!(parameter_oids(frames), vec![16], "{sql}");
        assert_eq!(result_oids(frames), vec![16], "{sql}");
        let expected_rows =
            expected.map_or_else(Vec::new, |text| vec![vec![Some(text.to_string())]]);
        assert_eq!(support::data_rows(frames), expected_rows, "{sql}");
    }
    for (sql, frames) in rejected.into_iter().zip(rejected_frames) {
        assert_eq!(
            support::error_code(frames).as_deref(),
            Some("42601"),
            "{sql}"
        );
        assert_eq!(
            support::data_rows(frames),
            Vec::<Vec<Option<String>>>::new(),
            "{sql}"
        );
        assert!(
            !frames.iter().any(|(tag, _)| *tag == b'T'),
            "reject before Describe metadata: {sql}"
        );
    }
}

#[test]
fn should_preserve_boolean_parameter_types_across_warm_describe_orders() {
    // Arrange
    let orders = [
        ("text-first", [25, 25, 16, 25]),
        ("boolean-first", [16, 25, 25, 16]),
    ];
    for (order_label, oids) in orders {
        for literal in ["no", "maybe"] {
            let sql = format!("SELECT $1 = '{literal}' AS matched");
            let cycles = oids.map(|oid| text_cycle(&sql, oid, literal)).to_vec();

            // Act
            let batches = run_cycles(&format!("bool-wire-warm-{order_label}-{literal}"), cycles);

            // Assert
            for (oid, frames) in oids.into_iter().zip(batches) {
                if oid == 16 && literal == "maybe" {
                    assert_eq!(support::error_code(&frames).as_deref(), Some("42601"));
                    assert_eq!(
                        support::data_rows(&frames),
                        Vec::<Vec<Option<String>>>::new()
                    );
                    assert!(!frames.iter().any(|(tag, _)| *tag == b'T'));
                    continue;
                }
                assert_eq!(support::error_code(&frames), None, "OID {oid}: {sql}");
                assert_eq!(parameter_oids(&frames), vec![oid], "{sql}");
                assert_eq!(result_oids(&frames), vec![16], "{sql}");
                assert_eq!(
                    support::data_rows(&frames),
                    vec![vec![Some("t".to_owned())]],
                    "{sql}"
                );
            }
        }
    }
}

#[test]
fn should_preserve_declared_boolean_oids_for_null_execution_without_describe() {
    // Arrange
    let cases = [
        (16, "no", None),
        (16, "maybe", Some("42601")),
        (25, "maybe", None),
    ];
    let cycles = cases
        .map(|(oid, literal, _)| {
            vec![
                support::parse_frame_with_types(
                    "",
                    &format!("SELECT $1 = '{literal}' AS matched"),
                    &[oid],
                ),
                support::bind_frame_with_formats("", "", &[0], &[None], &[]),
                support::execute_frame(""),
                support::sync_frame(),
            ]
        })
        .to_vec();

    // Act
    let batches = run_cycles("bool-wire-null-without-describe", cycles);

    // Assert
    for ((oid, literal, expected_error), frames) in cases.into_iter().zip(batches) {
        assert_eq!(
            support::error_code(&frames).as_deref(),
            expected_error,
            "OID {oid}: {literal}"
        );
        assert!(
            !frames.iter().any(|(tag, _)| *tag == b't'),
            "no Describe was sent"
        );
        if expected_error.is_some() {
            assert_eq!(
                support::data_rows(&frames),
                Vec::<Vec<Option<String>>>::new()
            );
            assert!(!frames.iter().any(|(tag, _)| *tag == b'T'));
        } else {
            assert_eq!(result_oids(&frames), vec![16]);
            assert_eq!(support::data_rows(&frames), vec![vec![None]]);
        }
    }
}

#[test]
fn should_validate_exported_parameter_types_in_boolean_scopes() {
    // Arrange
    let cases = [
        ("SELECT 1 FROM (SELECT $1 AS flag) AS d WHERE d.flag", false),
        (
            "WITH c AS (SELECT $1 AS flag) SELECT 1 FROM c WHERE flag",
            false,
        ),
        (
            "SELECT d.flag = 'no' AS matched FROM (SELECT $1 AS flag) AS d",
            true,
        ),
        (
            "WITH c AS (SELECT $1 AS flag) SELECT flag = 'no' AS matched FROM c",
            true,
        ),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(sql, _)| {
            [
                text_cycle(sql, 16, "no"),
                vec![
                    support::parse_frame_with_types("", sql, &[25]),
                    support::describe_statement_frame(""),
                    support::bind_frame_with_formats("", "", &[0], &[None], &[]),
                    support::execute_frame(""),
                    support::sync_frame(),
                ],
            ]
        })
        .collect();

    // Act
    let batches = run_cycles("bool-wire-exported-parameter-types", cycles);

    // Assert
    for ((sql, comparison), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        let accepted = &pair[0];
        let errors = accepted
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| support::parse_error_fields(payload))
            .collect::<Vec<_>>();
        assert_eq!(support::error_code(accepted), None, "{sql}: {errors:?}");
        assert_eq!(parameter_oids(accepted), vec![16], "{sql}");
        let expected = if comparison {
            vec![vec![Some("t".to_string())]]
        } else {
            vec![]
        };
        assert_eq!(support::data_rows(accepted), expected, "{sql}");
        assert_eq!(
            result_oids(accepted),
            vec![if comparison { 16 } else { 23 }],
            "{sql}"
        );
        let text_null = &pair[1];
        if comparison {
            assert_eq!(
                support::error_code(text_null),
                None,
                "TEXT can compare with an unknown string: {sql}"
            );
            assert_eq!(result_oids(text_null), vec![16], "{sql}");
            assert_eq!(support::data_rows(text_null), vec![vec![None]], "{sql}");
        } else {
            assert_eq!(
                support::error_code(text_null).as_deref(),
                Some("42601"),
                "typed TEXT predicate remains TEXT when bound NULL: {sql}"
            );
            assert_eq!(
                support::data_rows(text_null),
                Vec::<Vec<Option<String>>>::new(),
                "{sql}"
            );
            assert!(
                !text_null.iter().any(|(tag, _)| *tag == b'T'),
                "reject before metadata: {sql}"
            );
        }
    }
}

#[test]
fn should_preserve_decoded_numeric_oid_boolean_validation() {
    // Arrange
    let numeric_inputs = [(700, "2.5"), (1700, "2"), (1700, "2.5")];
    let cycles = numeric_inputs
        .iter()
        .flat_map(|(oid, text)| {
            [
                vec![
                    support::parse_frame_with_types(
                        "",
                        "SELECT 1 WHERE coalesce($1, TRUE)",
                        &[*oid],
                    ),
                    support::bind_frame_with_formats("", "", &[0], &[Some(text.as_bytes())], &[]),
                    support::execute_frame(""),
                    support::sync_frame(),
                ],
                text_cycle("SELECT CAST($1 AS BOOLEAN) AS flag", *oid, text),
            ]
        })
        .collect::<Vec<_>>();

    // Act
    let batches = run_cycles("bool-wire-decoded-numeric-oids", cycles);

    // Assert
    assert_eq!(batches.len(), numeric_inputs.len() * 2);
    for ((oid, text), pair) in numeric_inputs.into_iter().zip(batches.as_chunks::<2>().0) {
        let rejected = &pair[0];
        assert_eq!(
            support::error_code(rejected).as_deref(),
            Some("42601"),
            "numeric OID {oid}, input {text} must retain its decoded type"
        );
        assert_eq!(
            support::data_rows(rejected),
            Vec::<Vec<Option<String>>>::new()
        );
        assert!(!rejected.iter().any(|(tag, _)| *tag == b'T'));
        assert!(
            !rejected.iter().any(|(tag, _)| *tag == b't'),
            "rejected cycle omits Describe"
        );
        let cast = &pair[1];
        assert_eq!(support::error_code(cast), None, "numeric OID {oid}");
        assert_eq!(parameter_oids(cast), vec![oid]);
        assert_eq!(result_oids(cast), vec![16]);
        assert_eq!(support::data_rows(cast), vec![vec![Some("t".to_owned())]]);
    }
}

#[test]
fn should_infer_unknown_boolean_insert_select_destination_parameters() {
    // Arrange
    let empty_sql =
        "INSERT INTO bool_wire (id, n, flag) SELECT 2, 20, $1 FROM bool_wire_empty RETURNING flag";
    let populated_zero_sql = "INSERT INTO bool_wire (id, n, flag) SELECT 2, 20, $1 FROM bool_wire WHERE id = 1 RETURNING flag";
    let populated_unknown_sql = "INSERT INTO bool_wire (id, n, flag) SELECT 3, 30, $1 FROM bool_wire WHERE id = 1 RETURNING flag";
    let cases = [
        (empty_sql, 0, false),
        (empty_sql, 705, false),
        (populated_zero_sql, 0, true),
        (populated_unknown_sql, 705, true),
        (empty_sql, 25, false),
        ("INSERT INTO bool_wire (id, n, flag) SELECT 4, 40, $1 FROM bool_wire WHERE id = 1 RETURNING flag", 25, true),
    ];
    let mut cycles = cases
        .iter()
        .map(|(sql, oid, _)| text_cycle(sql, *oid, "no"))
        .collect::<Vec<_>>();
    cycles.push(vec![
        support::parse_frame_with_types(
            "",
            "SELECT id, flag FROM bool_wire WHERE id IN (2, 3, 4) ORDER BY id",
            &[],
        ),
        support::describe_statement_frame(""),
        support::bind_frame("", "", &[]),
        support::execute_frame(""),
        support::sync_frame(),
    ]);

    // Act
    let batches = run_cycles("bool-wire-insert-select-unknown-destination", cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() + 1);
    for ((sql, supplied_oid, populated), frames) in cases.into_iter().zip(&batches) {
        if supplied_oid == 25 {
            assert_eq!(
                support::error_code(frames).as_deref(),
                Some("42601"),
                "typed TEXT stays TEXT: {sql}"
            );
            assert_eq!(
                support::data_rows(frames),
                Vec::<Vec<Option<String>>>::new(),
                "{sql}"
            );
            assert!(
                !frames.iter().any(|(tag, _)| *tag == b'T'),
                "typed TEXT rejects before result metadata: {sql}"
            );
        } else {
            assert_eq!(
                support::error_code(frames),
                None,
                "{sql}, OID {supplied_oid}"
            );
            assert_eq!(
                parameter_oids(frames),
                vec![16],
                "destination slot infers BOOLEAN: {sql}"
            );
            assert_eq!(
                result_oids(frames),
                vec![16],
                "RETURNING stays BOOLEAN: {sql}"
            );
            let expected = if populated {
                vec![vec![Some("f".to_string())]]
            } else {
                Vec::new()
            };
            assert_eq!(
                support::data_rows(frames),
                expected,
                "{sql}, OID {supplied_oid}"
            );
        }
    }
    let stored = batches.last().expect("stored-value verification");
    assert_eq!(support::error_code(stored), None);
    assert_eq!(
        support::data_rows(stored),
        vec![
            vec![Some("2".to_string()), Some("f".to_string())],
            vec![Some("3".to_string()), Some("f".to_string())],
        ]
    );
}
