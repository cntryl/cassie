//! Native row predicate errors, descriptors and statement recovery.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_reject_row_arity_given_mismatched_constructors_when_queries_are_parsed() {
    // Arrange
    let cases = [
        "SELECT ROW(1) = ROW(1, 2)",
        "SELECT (1, 2) < ROW(1)",
        "SELECT ROW() IS DISTINCT FROM ROW(1)",
    ];
    // Act
    let outcomes = cases.map(|sql| {
        run_script(
            "row-arity",
            vec![
                Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
                Step::ThroughReady(vec![wire::simple_query_frame("SELECT 1")]),
            ],
        )
    });
    // Assert
    for batches in outcomes {
        assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42601"));
        let error = batches[0]
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("row arity diagnostic");
        assert!(wire::parse_error_fields(&error.1)
            .iter()
            .any(|(kind, message)| *kind == 'M'
                && message.contains("unequal number of entries in row expressions")));
        assert_eq!(
            wire::data_rows(&batches[0]),
            Vec::<Vec<Option<String>>>::new()
        );
        assert_eq!(wire::error_code(&batches[1]), None);
        assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("1".into())]]);
    }
}

#[test]
fn should_reject_empty_ordered_rows_given_native_operators_when_connections_are_reused() {
    // Arrange
    let operators = ["=", "<>", "<", "<=", ">", ">="];
    // Act
    let outcomes = operators.map(|op| {
        run_script(
            "row-empty-errors",
            vec![
                Step::ThroughReady(vec![wire::simple_query_frame(&format!(
                    "SELECT ROW() {op} ROW()"
                ))]),
                Step::ThroughReady(vec![wire::simple_query_frame(
                    "SELECT ROW() IS NOT DISTINCT FROM ROW()",
                )]),
            ],
        )
    });
    // Assert
    for batches in outcomes {
        assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
        assert_eq!(wire::error_code(&batches[1]), None);
        assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("t".into())]]);
    }
}

#[test]
fn should_preserve_constant_errors_given_later_row_fields_when_no_output_rows_are_requested() {
    // Arrange
    let cases = [
        "SELECT (1, 1 / 0) < (2, 0)",
        "SELECT (1, 1 / 0) < (2, 0) WHERE FALSE",
        "SELECT (1, 1 / 0) < (2, 0) LIMIT 0",
        "SELECT CASE WHEN (1,2)<(1,3) THEN 1 ELSE 1/0 END",
        "SELECT CASE WHEN CAST((1,2)<(1,3) AS BOOLEAN) THEN 1 ELSE 1/0 END",
        "SELECT CASE WHEN NOT((1,2)>(1,3)) THEN 1 ELSE 1/0 END",
        "SELECT CASE WHEN ROW((1,2)<(1,3))=ROW(TRUE) THEN TRUE ELSE (1/0)=0 END",
    ];
    // Act
    let outcomes = cases.map(|sql| {
        run_script(
            "row-constant-errors",
            vec![
                Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
                Step::ThroughReady(vec![wire::simple_query_frame("SELECT (1, NULL) < (2, 0)")]),
            ],
        )
    });
    // Assert
    for batches in outcomes {
        assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22012"));
        assert_eq!(
            wire::data_rows(&batches[0]),
            Vec::<Vec<Option<String>>>::new()
        );
        assert_eq!(wire::error_code(&batches[1]), None);
        assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("t".into())]]);
    }
}

#[test]
fn should_retain_parameter_types_given_row_comparisons_when_binary_portals_are_rebound() {
    // Arrange
    let batches = run_script(
        "row-parameter-binary",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "p",
                "SELECT ($1, $2) < ($3, $4) AS p",
                &[23, 23, 23, 23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("p"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "a",
                    "p",
                    &[],
                    &[Some(b"1"), None, Some(b"2"), Some(b"0")],
                    &[1],
                ),
                wire::describe_portal_frame("a"),
                wire::execute_frame("a"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "b",
                    "p",
                    &[],
                    &[Some(b"1"), None, Some(b"1"), Some(b"0")],
                    &[1],
                ),
                wire::execute_frame("b"),
                wire::sync_frame(),
            ]),
        ],
    );
    // Act
    let parameters = batches[1].iter().find(|frame| frame.0 == b't');
    let description = batches[2].iter().find(|frame| frame.0 == b'T');
    let row = batches[2].iter().find(|frame| frame.0 == b'D');
    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::parse_parameter_description(&parameters.expect("ParameterDescription").1),
        vec![23; 4]
    );
    assert_eq!(
        wire::parse_row_description(&description.expect("Boolean description").1)[0].type_oid,
        16
    );
    assert_eq!(&row.expect("binary predicate result").1[6..], &[1]);
    assert_eq!(wire::data_rows(&batches[3]), vec![vec![None]]);
}

#[test]
fn should_infer_row_slots_independently_given_unknown_parameters_when_statements_are_described() {
    // Arrange
    let batches = run_script(
        "row-slot-types",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "p",
                "SELECT ($1, $2) = (1, TRUE)",
                &[0, 0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("p"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats("out", "p", &[], &[Some(b"1"), Some(b"true")], &[]),
                wire::execute_frame("out"),
                wire::sync_frame(),
            ]),
        ],
    );
    // Act
    let parameters = batches[1].iter().find(|frame| frame.0 == b't');
    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::parse_parameter_description(&parameters.expect("pairwise parameter description").1),
        vec![23, 16]
    );
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("t".into())]]);
}

#[test]
fn should_validate_later_field_types_given_decisive_prefixes_when_empty_results_are_requested() {
    // Arrange
    let cases = [
        "SELECT (1, 'x'::TEXT) < (2, 3)",
        "SELECT (1, TRUE) = (2, 1) WHERE FALSE",
    ];
    // Act
    let outcomes = cases.map(|sql| {
        run_script(
            "row-field-types",
            vec![
                Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
                Step::ThroughReady(vec![wire::simple_query_frame("SELECT ROW(1) = ROW(1)")]),
            ],
        )
    });
    // Assert
    for batches in outcomes {
        assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
        assert_eq!(wire::error_code(&batches[1]), None);
        assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("t".into())]]);
    }
}

#[test]
fn should_enforce_native_row_width_given_oversized_constructors_when_queries_are_reused() {
    // Arrange
    let fields = vec!["1"; 1665].join(",");
    let sql = format!("SELECT ROW({fields}) = ROW({fields})");
    // Act
    let batches = run_script(
        "row-width-limit",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(&sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT ROW(1) = ROW(1)")]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("54011"));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("t".into())]]);
}

#[test]
fn should_reject_conflicting_row_slot_types_given_repeated_parameters_when_statements_are_parsed() {
    // Arrange
    let frame = wire::parse_frame_with_types("p", "SELECT ROW($1,$1) = ROW(1,TRUE)", &[0]);
    // Act
    let batches = run_script(
        "row-slot-conflict",
        vec![
            Step::ParseFlush(frame),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT ROW(1) = ROW(1)")]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42P08"));
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![Some("t".into())]]);
}

#[test]
fn should_preserve_integer_field_domains_given_row_predicates_when_constant_overflow_is_planned() {
    // Arrange
    let cases = [
        "SELECT ROW(2147483647 + 1) = ROW(0)",
        "SELECT ROW(32767::SMALLINT + 1::SMALLINT) = ROW(0::BIGINT)",
    ];
    // Act
    let outcomes = cases.map(|sql| {
        run_script(
            "row-integer-overflow",
            vec![
                Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
                Step::ThroughReady(vec![wire::simple_query_frame("SELECT ROW(1) = ROW(1)")]),
            ],
        )
    });
    // Assert
    for batches in outcomes {
        assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
        assert_eq!(wire::error_code(&batches[1]), None);
        assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("t".into())]]);
    }
}

#[test]
fn should_preserve_row_predicate_descriptors_given_persisted_views_when_the_database_restarts() {
    // Arrange
    let setup = [(
        "CREATE VIEW row_predicate_view AS SELECT (1, NULL) < (2, 0) AS p",
        vec![],
    )];
    let stages = (0..2)
        .map(|_| {
            vec![vec![wire::simple_query_frame(
                "SELECT p FROM row_predicate_view",
            )]]
        })
        .collect();
    // Act
    let outcomes = crate::support_type_metadata_contract::run_cycles_across_restart(
        "row-predicate-view-restart",
        &setup,
        stages,
    );
    // Assert
    assert_eq!(outcomes.len(), 2);
    for stage in outcomes {
        for frames in stage {
            assert_eq!(wire::error_code(&frames), None);
            let description = frames
                .iter()
                .find(|frame| frame.0 == b'T')
                .expect("view description");
            assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
            assert_eq!(wire::data_rows(&frames), vec![vec![Some("t".into())]]);
        }
    }
}

#[test]
fn should_fold_single_values_source_fields_given_row_predicates_when_constant_errors_are_planned() {
    // Arrange
    let sql = "SELECT ROW(a, 1/b) < ROW(a+1, 0) FROM (VALUES (1,0)) AS v(a,b)";
    // Act
    let batches = run_script(
        "row-single-values-folding",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT ROW(1) < ROW(2)")]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22012"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("t".into())]]);
}
