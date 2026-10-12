//! Production Parse qualification for borrowed primitive function signatures.

use std::mem::size_of;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::catalog::{Catalog, FunctionArgMeta, FunctionMeta, Volatility};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::{
    checked_parse_parameter_types_in_scope, parse_statement, AdmittedParameterTypes,
    ParameterAdmissionError,
};
use crate::types::DataType;

use super::FunctionInputScope;

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

fn register_function(catalog: &Catalog, name: &str, input: DataType) {
    catalog.register_function(FunctionMeta {
        name: name.to_owned(),
        args: vec![FunctionArgMeta {
            name: "value".to_owned(),
            data_type: input,
        }],
        return_type: DataType::Text,
        volatility: Volatility::Immutable,
        body: "CAST(value AS TEXT)".to_owned(),
    });
}

fn scoped_catalog() -> Catalog {
    let catalog = Catalog::new();
    for namespace in ["db.path_a", "db.path_b", "db.reader", "other.path_a"] {
        catalog.register_namespace(namespace, None);
    }
    for (name, input) in [
        ("db.public.pick", DataType::Int),
        ("db.path_a.pick", DataType::Text),
        ("db.path_b.pick", DataType::Boolean),
        ("db.reader.pick", DataType::BigInt),
        ("other.path_a.pick", DataType::Boolean),
    ] {
        register_function(&catalog, name, input);
    }
    catalog
}

#[test]
fn should_resolve_function_inputs_in_session_scope() {
    // Arrange
    let catalog = scoped_catalog();
    let cases: &[(&str, &[&str], i32)] = &[
        ("pick", &["path_a", "path_b"], 25),
        ("pick", &["path_b", "path_a"], 16),
        ("pick", &["$user", "path_a"], 20),
        ("pick", &["missing", "$user"], 20),
        ("pick", &["missing"], 23),
        ("pick", &[], 23),
        ("pick", &[" path_a "], 23),
        ("pick", &["PATH_B"], 16),
        ("path_b.pick", &["path_a"], 16),
        ("db.path_b.pick", &["path_a"], 16),
        ("other.path_a.pick", &["path_a"], 705),
    ];

    // Act
    let observations = cases
        .iter()
        .map(|(name, path, _)| {
            let search_path = path
                .iter()
                .map(|entry| (*entry).to_owned())
                .collect::<Vec<_>>();
            let scope = FunctionInputScope {
                database: "db",
                search_path: &search_path,
                user: "reader",
                enforce_database_scope: true,
            };
            let statement = parse_statement(&format!("SELECT {name}($1)")).expect("fixture SQL");
            let controls = controls(128 * 1_024);
            let admitted = checked_parse_parameter_types_in_scope(
                &statement,
                &[705],
                &catalog,
                &controls,
                Some(&scope),
            )
            .expect("scoped signature admission");
            let live_bytes = controls.current_query_memory_bytes();
            let types = admitted.into_vec().expect("final Parse frontier");
            (types, live_bytes, controls.current_query_memory_bytes())
        })
        .collect::<Vec<_>>();

    // Assert
    for ((name, path, expected), (types, live_bytes, released_bytes)) in
        cases.iter().zip(observations)
    {
        assert_eq!(types, [*expected], "{name} in {path:?}");
        assert_eq!(live_bytes, size_of::<i32>(), "{name} in {path:?}");
        assert_eq!(released_bytes, 0, "{name} in {path:?}");
    }
}

#[test]
fn should_retain_only_admitted_oids_after_function_lookup() {
    // Arrange
    let catalog = scoped_catalog();
    let search_path = vec!["path_a".to_owned()];
    let scope = FunctionInputScope {
        database: "db",
        search_path: &search_path,
        user: "reader",
        enforce_database_scope: true,
    };
    let statement = parse_statement("SELECT pick($1)").expect("fixture SQL");
    let controls = controls(128 * 1_024);

    // Act
    let admitted = checked_parse_parameter_types_in_scope(
        &statement,
        &[705],
        &catalog,
        &controls,
        Some(&scope),
    )
    .expect("scoped signature admission");

    // Assert
    assert_eq!(admitted.types(), [25]);
    assert_eq!(controls.current_query_memory_bytes(), size_of::<i32>());
    assert!(controls.peak_query_memory_bytes() > size_of::<i32>());
    drop(admitted);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_function_lookup_scratch_after_budget_denial() {
    // Arrange
    let name = "wide_function_".repeat(32);
    let statement = parse_statement(&format!("SELECT {name}($1)")).expect("fixture SQL");
    let search_path = vec!["public".to_owned()];
    let scope = FunctionInputScope {
        database: "db",
        search_path: &search_path,
        user: "reader",
        enforce_database_scope: true,
    };
    let baseline_controls = controls(128 * 1_024);
    let baseline = checked_parse_parameter_types_in_scope(
        &statement,
        &[705],
        &Catalog::new(),
        &baseline_controls,
        Some(&scope),
    )
    .expect("same Parse traversal without a stored signature");
    assert_eq!(baseline.types(), [705]);
    drop(baseline);
    let budget = baseline_controls.peak_query_memory_bytes();
    let catalog = Catalog::new();
    register_function(&catalog, &format!("db.public.{name}"), DataType::Text);
    let controls = controls(budget);

    // Act
    let result = checked_parse_parameter_types_in_scope(
        &statement,
        &[705],
        &catalog,
        &controls,
        Some(&scope),
    );

    // Assert
    assert!(
        matches!(result, Err(ParameterAdmissionError::Memory(_))),
        "{result:?}"
    );
    assert!(controls.peak_query_memory_bytes() > 0);
    assert!(controls.peak_query_memory_bytes() <= budget);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(baseline_controls.current_query_memory_bytes(), 0);
}

fn wide_function_statement(
    catalog: &Catalog,
) -> (crate::sql::ParsedStatement, QueryExecutionControls) {
    let search_path = vec!["public".to_owned()];
    let scope = FunctionInputScope {
        database: "db",
        search_path: &search_path,
        user: "reader",
        enforce_database_scope: true,
    };
    let narrow_controls = controls(128 * 1_024);
    let narrow_statement = parse_statement("SELECT pick($1)").expect("narrow fixture SQL");
    let narrow = checked_parse_parameter_types_in_scope(
        &narrow_statement,
        &[705],
        catalog,
        &narrow_controls,
        Some(&scope),
    )
    .expect("uncontended signature baseline");
    assert_eq!(narrow.types(), [25]);
    drop(narrow);
    let projection = std::iter::repeat_n("$1", 512)
        .chain(std::iter::once("pick($1)"))
        .collect::<Vec<_>>()
        .join(", ");
    let wide_statement =
        parse_statement(&format!("SELECT {projection}")).expect("513 valid projection columns");
    (wide_statement, narrow_controls)
}

#[test]
fn should_cancel_parameter_inference_while_the_function_catalog_is_locked() {
    // Arrange
    let catalog = Catalog::new();
    register_function(&catalog, "db.public.pick", DataType::Text);
    let (wide_statement, narrow_controls) = wide_function_statement(&catalog);
    let narrow_peak = narrow_controls.peak_query_memory_bytes();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 128 * 1_024,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation.clone(),
    );
    let worker_catalog = catalog.clone();
    let worker_controls = controls.clone();
    let writer = catalog.functions.write();
    let (completed, completion) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let search_path = vec!["public".to_owned()];
        let scope = FunctionInputScope {
            database: "db",
            search_path: &search_path,
            user: "reader",
            enforce_database_scope: true,
        };
        let result = checked_parse_parameter_types_in_scope(
            &wide_statement,
            &[705],
            &worker_catalog,
            &worker_controls,
            Some(&scope),
        );
        let cancelled = matches!(result, Err(ParameterAdmissionError::Cancelled));
        drop(result);
        let _ = completed.send((cancelled, worker_controls.current_query_memory_bytes()));
    });
    // A wide live charge above the complete narrow peak proves admitted progress.
    // It does not identify the exact instruction of the next catalog read attempt.
    let wait_deadline = Instant::now() + Duration::from_secs(2);
    while controls.current_query_memory_bytes() <= narrow_peak && Instant::now() < wait_deadline {
        thread::sleep(Duration::from_millis(1));
    }
    let reached_wide_admission = controls.current_query_memory_bytes() > narrow_peak;

    // Act
    cancellation.cancel();
    let completed_while_locked = completion.recv_timeout(Duration::from_secs(2));
    drop(writer);
    // Always release the writer and retire the worker before any assertion, including failures.
    let completed_before_release = completed_while_locked.is_ok();
    let outcome =
        completed_while_locked.or_else(|_| completion.recv_timeout(Duration::from_secs(2)));
    let joined = worker.join();

    // Assert
    assert!(
        reached_wide_admission,
        "worker did not retain more metadata than the narrow signature baseline"
    );
    assert!(
        completed_before_release,
        "cancellation waited for the catalog writer"
    );
    assert!(joined.is_ok(), "parameter-inference worker panicked");
    assert_eq!(
        outcome.expect("retired parameter-inference worker"),
        (true, 0)
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(narrow_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_expired_function_input_lookup_without_retaining_memory() {
    // Arrange
    let catalog = scoped_catalog();
    let search_path = vec!["path_a".to_owned()];
    let scope = FunctionInputScope {
        database: "db",
        search_path: &search_path,
        user: "reader",
        enforce_database_scope: true,
    };
    let statement = parse_statement("SELECT pick($1)").expect("fixture SQL");
    let mut controls = controls(128 * 1_024);
    controls.deadline = Some(
        Instant::now()
            .checked_sub(Duration::from_secs(1))
            .expect("past deadline"),
    );

    // Act
    let result = checked_parse_parameter_types_in_scope(
        &statement,
        &[705],
        &catalog,
        &controls,
        Some(&scope),
    );

    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Deadline)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}

#[test]
fn should_skip_input_metadata_for_functions_without_arguments() {
    // Arrange
    let catalog = Catalog::new();
    let search_path = vec!["missing_schema_".repeat(32)];
    let scope = FunctionInputScope {
        database: "db",
        search_path: &search_path,
        user: "reader",
        enforce_database_scope: true,
    };
    let statements = [
        "SELECT CAST($1 AS INT)",
        "SELECT version(), CAST($1 AS INT)",
    ];

    // Act
    let observations = statements.map(|sql| {
        let statement = parse_statement(sql).expect("existing syntax");
        let controls = controls(1_024);
        let result = checked_parse_parameter_types_in_scope(
            &statement,
            &[705],
            &catalog,
            &controls,
            Some(&scope),
        )
        .and_then(AdmittedParameterTypes::into_vec);
        (result, controls.current_query_memory_bytes())
    });

    // Assert
    for (sql, (result, retained)) in statements.into_iter().zip(observations) {
        assert_eq!(result.expect(sql), [23], "{sql}");
        assert_eq!(retained, 0, "{sql}");
    }
}
