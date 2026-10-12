//! Qualification of the new occurrence and phase owners on real shared controls.

use std::mem::size_of;
use std::time::Instant;

use crate::catalog::Catalog;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::sql::parse_statement;

use super::{checked_parse_parameter_types, ParameterAdmissionError};

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

#[test]
fn should_bound_repeated_parameter_occurrences_on_shared_controls() {
    // Arrange
    let wide = parse_statement(&format!(
        "SELECT {}",
        std::iter::repeat_n("$1", 512)
            .collect::<Vec<_>>()
            .join(", ")
    ))
    .expect("512 admitted projection columns");
    let narrow = parse_statement("SELECT $1").expect("one occurrence");
    let catalog = Catalog::new();
    let narrow_controls = controls(1_024 * 1_024);
    let narrow_result = checked_parse_parameter_types(&narrow, &[23], &catalog, &narrow_controls)
        .expect("narrow control");
    let narrow_oids = narrow_result.types().to_vec();
    let narrow_peak = narrow_controls.peak_query_memory_bytes();
    let narrow_retained = narrow_controls.current_query_memory_bytes();
    drop(narrow_result);
    let narrow_released = narrow_controls.current_query_memory_bytes();
    let wide_controls = controls(1_024 * 1_024);
    let limited = controls(narrow_peak);
    // Act
    let wide_result = checked_parse_parameter_types(&wide, &[23], &catalog, &wide_controls)
        .expect("wide control");
    let wide_oids = wide_result.types().to_vec();
    let wide_peak = wide_controls.peak_query_memory_bytes();
    let wide_retained = wide_controls.current_query_memory_bytes();
    drop(wide_result);
    let wide_released = wide_controls.current_query_memory_bytes();
    let denied = checked_parse_parameter_types(&wide, &[23], &catalog, &limited);
    let denied_peak = limited.peak_query_memory_bytes();
    let denied_released = limited.current_query_memory_bytes();
    let reuse = limited
        .reserve_query_memory(narrow_peak)
        .expect("reuse whole denied budget");
    drop(reuse);
    let again = checked_parse_parameter_types(&wide, &[23], &catalog, &wide_controls)
        .expect("denial does not poison shared state");
    let again_oids = again.types().to_vec();
    drop(again);
    // Assert
    assert_eq!(narrow_oids, [23]);
    assert_eq!(wide_oids, [23]);
    assert_eq!(again_oids, [23]);
    assert_eq!(narrow_retained, size_of::<i32>());
    assert_eq!(wide_retained, size_of::<i32>());
    assert!(wide_peak > narrow_peak);
    assert!(matches!(denied, Err(ParameterAdmissionError::Memory(_))));
    assert!(denied_peak <= narrow_peak);
    assert_eq!((narrow_released, wide_released, denied_released), (0, 0, 0));
    assert_eq!(limited.current_query_memory_bytes(), 0);
    assert_eq!(wide_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_diagnostic_formatting_by_source_derived_capacities() {
    // Arrange
    use super::diagnostics::{BooleanConstruct, ParameterSemanticDiagnostic};
    let diagnostics = [
        ParameterSemanticDiagnostic::Inconsistent {
            slot: 268_435_454,
            previous: 16,
            requested: 25,
        },
        ParameterSemanticDiagnostic::Indeterminate { slot: 268_435_454 },
        ParameterSemanticDiagnostic::Indeterminate { slot: usize::MAX },
        ParameterSemanticDiagnostic::BooleanOperand {
            construct: BooleanConstruct::Where,
            actual: 1043,
        },
    ];
    // Act
    let formatted = diagnostics.map(|diagnostic| {
        let mut message = String::with_capacity(diagnostic.message_capacity());
        diagnostic
            .write_message(&mut message)
            .expect("bounded message formatting");
        let mut detail = diagnostic.detail_capacity().map(String::with_capacity);
        if let Some(detail) = &mut detail {
            diagnostic
                .write_detail(detail)
                .expect("bounded detail formatting");
        }
        (diagnostic, message, detail)
    });
    // Assert
    for (diagnostic, message, detail) in formatted {
        assert!(message.len() <= diagnostic.message_capacity());
        assert_eq!(message, diagnostic.message());
        assert_eq!(detail, diagnostic.detail());
        if let Some(detail) = detail {
            assert_eq!(
                detail.len(),
                diagnostic.detail_capacity().expect("detail bound")
            );
        }
    }
}

#[test]
fn should_preserve_legacy_conflicts_for_operator_inferred_slots() {
    // Arrange
    let statements = [
        "SELECT 1 WHERE ($1 AND TRUE) AND ($1 = 1)",
        "SELECT 1 WHERE ($1 = 1) AND ($1 AND TRUE)",
    ];
    let catalog = Catalog::new();
    let controls = controls(1_024 * 1_024);

    // Act
    let outcomes = statements.map(|sql| {
        let parsed = parse_statement(sql).expect("existing mixed-context syntax");
        [0, 705].map(|oid| {
            let result = checked_parse_parameter_types(&parsed, &[oid], &catalog, &controls);
            let released = controls.current_query_memory_bytes();
            (result, released)
        })
    });

    // Assert
    for outcomes in outcomes {
        for (result, released) in outcomes {
            let error = result.expect_err("retain the legacy mixed-context conflict");
            assert!(matches!(
                error,
                ParameterAdmissionError::LegacyConflict(ref message)
                    if message == "parameter $1 has incompatible boolean and numeric contexts"
            ));
            assert_eq!(released, 0);
        }
    }
    assert!(controls.peak_query_memory_bytes() > 0);
}

#[test]
fn should_preserve_operator_inferred_metadata_before_target_finalization() {
    // Arrange
    let statements = [
        "SELECT $1, $1 = 1",
        "SELECT $1, $1 + 1",
        "SELECT $1 IS NULL, $1 + 1",
    ];
    let catalog = Catalog::new();
    let controls = controls(1_024 * 1_024);

    // Act
    let outcomes = statements.map(|sql| {
        let parsed = parse_statement(sql).expect("existing operator syntax");
        let admitted = checked_parse_parameter_types(&parsed, &[0], &catalog, &controls)
            .expect("retain contextual operator inference");
        let oids = admitted.types().to_vec();
        let retained = controls.current_query_memory_bytes();
        drop(admitted);
        (oids, retained, controls.current_query_memory_bytes())
    });

    // Assert
    for (oids, retained, released) in outcomes {
        assert_eq!(oids, [23]);
        assert_eq!(retained, size_of::<i32>());
        assert_eq!(released, 0);
    }
}

#[test]
fn should_preserve_selected_diagnostics_on_independent_parameter_slots() {
    // Arrange
    let cases = [
        (
            "SELECT 1 WHERE ($1 = 1) AND ($1 AND TRUE)",
            vec![23],
            "42804",
        ),
        ("SELECT 1 WHERE ($1 = 1) AND $2", vec![0, 25], "42804"),
        ("SELECT 1 WHERE $2 AND ($1 = 1)", vec![0, 25], "42804"),
        (
            "SELECT 1 FROM (SELECT 1 AS id) AS l JOIN (SELECT 1 AS id) AS r ON $1 = 1 WHERE $2",
            vec![0, 25],
            "42804",
        ),
        (
            "SELECT 1 FROM (SELECT 1 AS id HAVING $1 = 1) AS source WHERE $2",
            vec![0, 25],
            "42804",
        ),
        ("SELECT $1 = 1, $2, CAST($2 AS INT)", vec![0, 0], "42P08"),
    ];
    let catalog = Catalog::new();
    let controls = controls(1_024 * 1_024);

    // Act
    let outcomes = cases.map(|(sql, provided, expected)| {
        let parsed = parse_statement(sql).expect("selected sibling syntax");
        let result = checked_parse_parameter_types(&parsed, &provided, &catalog, &controls);
        (result, expected, controls.current_query_memory_bytes())
    });

    // Assert
    for (result, expected, released) in outcomes {
        let error = result.expect_err("selected diagnostics remain local to their slot");
        let ParameterAdmissionError::Semantic(diagnostic) = error else {
            panic!("selected semantic diagnostic required: {error:?}");
        };
        assert_eq!(diagnostic.code(), expected);
        assert_eq!(released, 0);
    }
}

#[test]
fn should_preserve_legacy_arithmetic_predicate_admission() {
    // Arrange
    let statement = parse_statement("SELECT 1 AS value WHERE $1 + 1")
        .expect("existing arithmetic predicate syntax");
    let catalog = Catalog::new();
    let controls = controls(1_024 * 1_024);

    // Act
    let outcomes = [0, 23].map(|provided| {
        let result = checked_parse_parameter_types(&statement, &[provided], &catalog, &controls)
            .map(|admitted| {
                let oids = admitted.types().to_vec();
                let retained = controls.current_query_memory_bytes();
                drop(admitted);
                (oids, retained)
            });
        (provided, result, controls.current_query_memory_bytes())
    });

    // Assert
    for (provided, outcome, released) in outcomes {
        let (oids, retained) = outcome.unwrap_or_else(|error| {
            panic!("OID {provided}: preserve legacy Parse admission before Describe: {error:?}")
        });
        assert_eq!(oids, [23]);
        assert_eq!(retained, size_of::<i32>());
        assert_eq!(released, 0);
    }
}

#[test]
fn should_preserve_legacy_array_case_parameter_identities() {
    // Arrange
    let statement =
        parse_statement("SELECT CASE WHEN TRUE THEN $1 ELSE CAST(NULL AS INT[]) END AS value")
            .expect("existing CASE array syntax");
    let catalog = Catalog::new();
    let controls = controls(1_024 * 1_024);

    // Act
    let checked = [0, 705].map(|provided| {
        let result = checked_parse_parameter_types(&statement, &[provided], &catalog, &controls)
            .map(|admitted| {
                let oids = admitted.types().to_vec();
                let retained = controls.current_query_memory_bytes();
                drop(admitted);
                (oids, retained)
            });
        (provided, result, controls.current_query_memory_bytes())
    });
    let public = [vec![], vec![705], vec![0]]
        .map(|provided| super::parameter_type_oids_with_catalog(&statement, &provided, &catalog));

    // Assert
    for (provided, outcome, released) in checked {
        let (oids, retained) = outcome.unwrap_or_else(|error| {
            panic!("OID {provided}: preserve legacy CASE parameter identity: {error:?}")
        });
        assert_eq!(oids, [34_023]);
        assert_eq!(retained, size_of::<i32>());
        assert_eq!(released, 0);
    }
    assert_eq!(public, [vec![34_023], vec![34_023], vec![0]]);
}

fn source_local_column_cases() -> [(&'static str, Vec<i32>, Vec<i32>); 15] {
    [
        ("SELECT COALESCE(postgres.public.fold_scope_bool.flag, $1) FROM postgres.public.fold_scope_bool", vec![705], vec![16]),
        ("SELECT COALESCE(public.fold_scope_bool.flag, $1) FROM postgres.public.fold_scope_bool", vec![705], vec![16]),
        ("SELECT COALESCE(fold_scope_bool.flag, $1) FROM postgres.public.fold_scope_bool", vec![705], vec![16]),
        ("SELECT COALESCE(postgres.public.fold_scope_int.flag, $1) FROM postgres.public.fold_scope_int", vec![705], vec![23]),
        ("SELECT COALESCE(public.fold_scope_int.flag, $1) FROM postgres.public.fold_scope_int", vec![705], vec![23]),
        ("SELECT COALESCE(fold_scope_int.flag, $1) FROM postgres.public.fold_scope_int", vec![705], vec![23]),
        ("SELECT COALESCE(o.flag, $1) FROM postgres.public.fold_scope_bool AS o JOIN postgres.public.fold_scope_int AS i ON TRUE", vec![705], vec![16]),
        ("SELECT COALESCE(i.flag, $1) FROM postgres.public.fold_scope_bool AS o JOIN postgres.public.fold_scope_int AS i ON TRUE", vec![705], vec![23]),
        ("SELECT CASE postgres.public.fold_scope_bool.flag WHEN $1 THEN 1 ELSE 0 END FROM postgres.public.fold_scope_bool", vec![705], vec![16]),
        ("SELECT CASE postgres.public.fold_scope_int.flag WHEN $1 THEN 1 ELSE 0 END FROM postgres.public.fold_scope_int", vec![705], vec![23]),
        ("SELECT COALESCE(\"d\".flag, $1) FROM (SELECT TRUE AS flag) AS \"d\"", vec![705], vec![16]),
        ("SELECT COALESCE(\"D\".flag, $1) FROM (SELECT TRUE AS flag) AS \"D\"", vec![705], vec![16]),
        ("SELECT COALESCE(\"d.d\".flag, $1) FROM (SELECT TRUE AS flag) AS \"d.d\"", vec![705], vec![16]),
        ("SELECT 1 FROM postgres.public.fold_scope_bool JOIN postgres.public.fold_scope_int ON TRUE WHERE fold_scope_bool.flag", vec![], vec![]),
        ("SELECT 1 FROM postgres.public.fold_scope_bool WHERE EXISTS (SELECT 1 FROM postgres.public.fold_scope_int WHERE postgres.public.fold_scope_bool.flag AND TRUE)", vec![], vec![]),
    ]
}

#[test]
fn should_preserve_source_local_column_types_during_parameter_admission() {
    // Arrange
    let catalog = Catalog::new();
    for (name, data_type) in [
        ("postgres.public.fold_scope_bool", crate::types::DataType::Boolean),
        ("postgres.public.fold_scope_int", crate::types::DataType::Int),
    ] {
        catalog.register_collection(name, vec![("flag".to_string(), data_type)]);
    }
    let controls = controls(1_024 * 1_024);

    // Act
    let outcomes = source_local_column_cases().map(|(sql, provided, expected)| {
        let statement = parse_statement(sql).expect("existing scoped column syntax");
        let outcome = checked_parse_parameter_types(&statement, &provided, &catalog, &controls)
            .map(|admitted| {
                let oids = admitted.types().to_vec();
                drop(admitted);
                oids
            });
        (sql, outcome, expected, controls.current_query_memory_bytes())
    });
    let own_integer = parse_statement("SELECT 1 FROM postgres.public.fold_scope_int WHERE public.fold_scope_int.flag")
        .expect("own qualified integer syntax");
    let rejected = checked_parse_parameter_types(&own_integer, &[], &catalog, &controls);
    let rejected_current = controls.current_query_memory_bytes();

    // Assert
    for (sql, outcome, expected, released) in outcomes {
        assert_eq!(outcome.unwrap_or_else(|error| panic!("{sql}: {error:?}")), expected);
        assert_eq!(released, 0, "{sql}");
    }
    assert!(matches!(rejected, Err(ParameterAdmissionError::Semantic(ref diagnostic)) if diagnostic.code() == "42804"));
    assert_eq!(rejected_current, 0);
    assert!(controls.peak_query_memory_bytes() > 0);
}
