use std::collections::HashMap;
use std::time::Instant;

use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryExecutionControls;

use super::{execute_query_source, BatchRow, Expr, JoinKind, QuerySource, SourceExecutionEnv};

#[test]
fn should_keep_join_source_output_charged_after_returning_to_the_consumer() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-join-source-ownership-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 1024 * 1024;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let source = QuerySource::Join {
        left: Box::new(QuerySource::SingleRow),
        right: Box::new(QuerySource::SingleRow),
        kind: JoinKind::Cross,
        on: Expr::BoolLiteral(true),
    };
    let mut cte_context = HashMap::new();

    // Act
    let (batches, text_fields) =
        execute_query_source(&env, &source, &mut cte_context, false, None, Some(1)).unwrap();

    // Assert
    assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 1);
    assert_eq!(text_fields, [] as [String; 0]);
    assert!(
        controls.current_query_memory_bytes() >= std::mem::size_of::<BatchRow>(),
        "retained returned join rows must own a reservation"
    );
    drop(batches);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_allocate_only_actual_row_slots_for_a_small_join_chunk() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-join-small-chunk-{}", uuid::Uuid::new_v4()));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 1024 * 1024;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let source = QuerySource::Join {
        left: Box::new(QuerySource::SingleRow),
        right: Box::new(QuerySource::SingleRow),
        kind: JoinKind::Cross,
        on: Expr::BoolLiteral(true),
    };
    let mut cte_context = HashMap::new();

    // Act
    let (batches, _) =
        execute_query_source(&env, &source, &mut cte_context, false, None, Some(1)).unwrap();

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 1);
    assert_eq!(batches[0].capacity(), 1);
    drop(batches);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_collection_source_body_charged_after_returning_to_a_loaded_join() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-loaded-collection-source-ownership-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 4 * 1024 * 1024;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone())
        .expect("loaded collection source fixture");
    cassie.startup().expect("bootstrap collection catalog");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE loaded_collection_source_guard (payload TEXT, arr INT[], extra JSON)",
            vec![],
        )
        .expect("loaded collection source table");
    let collection = crate::catalog::canonical_relation_name(
        "postgres",
        "public",
        "loaded_collection_source_guard",
    );
    cassie
        .midge
        .put_document(
            &collection,
            Some("row-0".to_owned()),
            serde_json::json!({
                "payload": "p".repeat(8192),
                "arr": [1, 2],
                "extra": {"retained": true},
            }),
        )
        .expect("full row with ARRAY metadata and JSON");
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let source = QuerySource::Collection(
        crate::sql::IdentifierPath::parse(&collection).expect("actual collection path"),
    );
    let mut cte_context = HashMap::new();

    // Act
    let (batches, _) = execute_query_source(&env, &source, &mut cte_context, true, None, Some(1))
        .expect("the complete qualified source fits its budget");

    // Assert
    assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 1);
    let row = &batches[0][0];
    let crate::types::Value::String(payload) =
        crate::executor::batch::RowAccess::get(row, "payload").expect("full TEXT payload")
    else {
        panic!("TEXT payload");
    };
    assert_eq!(payload.len(), 8192);
    assert!(
        crate::executor::batch::RowAccess::has_array_types(row),
        "full source preserves ARRAY metadata"
    );
    assert!(
        controls.current_query_memory_bytes() >= payload.capacity(),
        "returned source retains {} TEXT bytes while only {} stay admitted",
        payload.capacity(),
        controls.current_query_memory_bytes()
    );
    assert!(
        row.query_memory().is_some(),
        "source origin lease survives qualification"
    );
    drop(batches);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_admit_unleased_left_join_inputs_before_reading_the_right_source() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-unleased-left-source-admission-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 32 * 1024;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone())
        .expect("unleased left source fixture");
    let collection = poison_unleased_join_right_source(&cassie);
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let left = QuerySource::Cte("unleased_left".to_owned());
    let mut context = unleased_left_context();
    let (positive, _) = execute_query_source(&env, &left, &mut context, true, None, Some(1))
        .expect("the existing CTE source's serialized check fits the same budget");
    assert_eq!(positive.iter().map(Vec::len).sum::<usize>(), 1);
    assert!(positive[0][0].query_memory().is_none());
    assert!(
        super::accounting::cloned_row_bytes(&positive[0][0]).expect("complete input shape")
            > config.limits.query_memory_budget_bytes,
        "the existing complete input admission must reject before the RHS canary",
    );
    drop(positive);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let source = QuerySource::Join {
        left: Box::new(left),
        right: Box::new(QuerySource::Collection(
            crate::sql::IdentifierPath::parse(&collection).expect("right collection path"),
        )),
        kind: JoinKind::Cross,
        on: Expr::BoolLiteral(true),
    };
    let before_reads = cassie.midge.query_scan_entries_for_diagnostics();
    let before = cassie.runtime.snapshot();

    // Act
    let result = execute_query_source(&env, &source, &mut context, false, None, Some(1))
        .map_err(crate::app::CassieError::from);

    // Assert
    assert!(
        matches!(result, Err(crate::app::CassieError::ResourceLimit(_))),
        "left input admission must precede the right decode canary: {result:?}"
    );
    assert_eq!(
        cassie.midge.query_scan_entries_for_diagnostics(),
        before_reads
    );
    assert_eq!(
        cassie.runtime.snapshot().joins.executions,
        before.joins.executions
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

fn poison_unleased_join_right_source(cassie: &Cassie) -> String {
    const CANARY: &str = "unleased-right-source-decode-canary";
    cassie.startup().expect("bootstrap collection catalog");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE right_source_canary (payload TEXT)",
            vec![],
        )
        .expect("right source table");
    let collection =
        crate::catalog::canonical_relation_name("postgres", "public", "right_source_canary");
    cassie
        .midge
        .put_document(
            &collection,
            Some("one".to_owned()),
            serde_json::json!({"payload": CANARY}),
        )
        .expect("seed right source");
    let (key, _) = cassie
        .midge
        .raw_scan_prefix_for_collection(&collection, &[])
        .expect("right source RowStore entries")
        .into_iter()
        .find(|(_, value)| {
            value
                .windows(CANARY.len())
                .any(|bytes| bytes == CANARY.as_bytes())
        })
        .expect("unique right source payload");
    cassie
        .midge
        .raw_put(crate::midge::adapter::StorageFamily::Data, &key, &[u8::MAX])
        .expect("poison right source before its first decode");
    collection
}

fn unleased_left_context() -> super::CteContext {
    let entries = (0..128)
        .map(|index| (format!("field_{index:03}"), crate::types::Value::Int64(1)))
        .collect::<Vec<_>>();
    let fields = entries
        .iter()
        .map(|(name, _)| crate::types::FieldSchema {
            name: name.clone(),
            data_type: crate::types::DataType::BigInt,
            nullable: false,
        })
        .collect();
    HashMap::from([(
        "unleased_left".to_owned(),
        super::super::super::cte::CteRelation {
            rows: vec![entries],
            fields,
        },
    )])
}
