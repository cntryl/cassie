use crate::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;
use cassie::types::Value;

#[test]
fn should_publish_null_safe_views_with_selected_definition_semantics() {
    // Arrange
    let fixture = sql_fixture("versioned_null_safe_view", &[]);
    // Act
    fixture
        .execute("CREATE VIEW admitted AS SELECT NULL IS NOT DISTINCT FROM NULL AS same")
        .expect("selected persisted predicate");
    let rows = fixture.rows("SELECT same FROM admitted");
    let view = fixture
        .cassie
        .midge
        .get_view("admitted")
        .expect("metadata")
        .expect("view");
    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
    assert_eq!(view.definition_semantics_version, 1);
}

#[test]
fn should_preserve_constraint_defaults_after_a_rejected_null_safe_check() {
    // Arrange
    let fixture = sql_fixture("versioned_null_safe_check", &[]);
    fixture.execute("CREATE TABLE admitted_check (id BIGINT, n BIGINT CHECK (n IS DISTINCT FROM NULL), same BOOLEAN DEFAULT (NULL IS NOT DISTINCT FROM NULL))")
        .expect("selected simple CHECK and expression default");
    // Act
    let rejected = fixture.execute("INSERT INTO admitted_check(id,n) VALUES (1,NULL)");
    fixture
        .execute("INSERT INTO admitted_check(id,n) VALUES (2,7)")
        .expect("accepted row");
    let rows = fixture.rows("SELECT id,n,same FROM admitted_check");
    // Assert
    assert!(matches!(
        rejected.expect_err("NULL violates null-safe CHECK"),
        CassieError::CheckViolation { .. }
    ));
    assert_eq!(
        rows,
        vec![vec![Value::Int64(2), Value::Int64(7), Value::Bool(true)]]
    );
}

#[test]
fn should_reject_future_definition_versions_before_publication() {
    // Arrange
    let fixture = sql_fixture("future_null_safe_definition", &[]);
    let mut view = cassie::catalog::ViewMeta::new(
        "future_view",
        "SELECT TRUE AS same",
        cassie::types::Schema { fields: vec![] },
    );
    view.definition_semantics_version = 2;
    // Act
    let rejected = fixture.cassie.midge.put_view(&view);
    let stored = fixture
        .cassie
        .midge
        .get_view("future_view")
        .expect("readback");
    // Assert
    assert!(matches!(
        rejected.expect_err("unknown semantics"),
        CassieError::Unsupported(_)
    ));
    assert!(stored.is_none());
}

#[test]
fn should_preserve_null_safe_outer_join_multiplicity() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_join_bucket",
        &[
            "CREATE TABLE left_keys (id BIGINT, n BIGINT)",
            "CREATE TABLE right_keys (id BIGINT, n BIGINT)",
            "INSERT INTO left_keys VALUES (1,NULL),(2,NULL),(3,7),(4,9)",
            "INSERT INTO right_keys VALUES (10,NULL),(11,NULL),(12,7)",
        ],
    );
    // Act
    let rows = fixture.rows("SELECT l.id,r.id FROM left_keys l LEFT JOIN right_keys r ON l.n IS NOT DISTINCT FROM r.n ORDER BY l.id,r.id");
    let ordinary = fixture.rows("SELECT l.id,r.id FROM left_keys l JOIN right_keys r ON l.n = r.n");
    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(1), Value::Int64(11)],
            vec![Value::Int64(2), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(11)],
            vec![Value::Int64(3), Value::Int64(12)],
            vec![Value::Int64(4), Value::Null]
        ]
    );
    assert_eq!(ordinary, vec![vec![Value::Int64(3), Value::Int64(12)]]);
}

#[test]
fn should_preserve_null_safe_join_results_across_native_plans() {
    // Arrange
    let setup = [
        "CREATE TABLE native_left (id BIGINT,n BIGINT)",
        "CREATE TABLE native_right (id BIGINT,n BIGINT)",
        "INSERT INTO native_left VALUES (1,NULL),(2,NULL),(3,7),(4,9)",
        "INSERT INTO native_right VALUES (10,NULL),(11,NULL),(12,7)",
        "CREATE INDEX native_right_n ON native_right(n)",
    ];
    for native in [true, false] {
        let mut config = cassie::config::CassieRuntimeConfig::default();
        config.limits.execution_result_cache_enabled =
            cassie::config::ExecutionResultCacheEnabled::disabled();
        config.limits.vectorized_joins_enabled = native;
        config.limits.vectorized_join_batch_size = 2;
        let fixture = crate::support_sql_fixture::sql_fixture_with_config(
            "native_null_bucket",
            &setup,
            config,
        );
        // Act
        let rows = fixture.rows("SELECT l.id,r.id FROM native_left l JOIN native_right r ON l.n IS NOT DISTINCT FROM r.n ORDER BY l.id,r.id LIMIT 4");
        let metrics = fixture.cassie.metrics();
        // Assert
        assert_eq!(
            rows,
            vec![
                vec![Value::Int64(1), Value::Int64(10)],
                vec![Value::Int64(1), Value::Int64(11)],
                vec![Value::Int64(2), Value::Int64(10)],
                vec![Value::Int64(2), Value::Int64(11)]
            ]
        );
        assert_eq!(
            metrics["joins"]["last_strategy"],
            if native { "typed_hash" } else { "merge" }
        );
        assert_eq!(metrics["query"]["current_accounted_memory_bytes"], 0);
    }
}

#[test]
fn should_keep_null_safe_partial_indexes_current_after_writes() {
    // Arrange
    let fixture = sql_fixture(
        "versioned_partial_index",
        &[
            "CREATE TABLE indexed_definitions (id BIGINT,n BIGINT)",
            "INSERT INTO indexed_definitions VALUES(1,NULL),(2,7)",
            "CREATE INDEX admitted_partial ON indexed_definitions(n) WHERE n IS DISTINCT FROM NULL",
        ],
    );
    // Act
    fixture
        .execute("INSERT INTO indexed_definitions VALUES(3,7)")
        .expect("index maintenance");
    fixture
        .execute("UPDATE indexed_definitions SET n=NULL WHERE id=2")
        .expect("remove index membership");
    let rows = fixture.rows("SELECT id FROM indexed_definitions WHERE n=7 ORDER BY id");
    let index = fixture
        .cassie
        .midge
        .list_indexes()
        .expect("index metadata")
        .into_iter()
        .find(|index| index.name.ends_with("admitted_partial"))
        .expect("selected index");
    // Assert
    assert_eq!(index.definition_semantics_version, 1);
    assert_eq!(rows, vec![vec![Value::Int64(3)]]);
}

#[test]
fn should_preserve_legacy_definition_encoding() {
    // Arrange
    let view = cassie::catalog::ViewMeta::new(
        "legacy_view",
        "SELECT TRUE AS same",
        cassie::types::Schema { fields: vec![] },
    );
    // Act
    let encoded = serde_json::to_value(&view).expect("legacy encoding");
    // Assert
    assert!(encoded.get("definition_semantics_version").is_none());
}

#[test]
fn should_reject_legacy_null_safe_operator_admission() {
    // Arrange
    let fixture = sql_fixture("legacy_definition_version", &[]);
    let view = cassie::catalog::ViewMeta::new(
        "legacy_view",
        "SELECT NULL IS NOT DISTINCT FROM NULL AS same",
        cassie::types::Schema { fields: vec![] },
    );
    // Act
    let rejected = fixture.cassie.midge.put_view(&view);
    // Assert
    assert!(matches!(
        rejected.expect_err("legacy admission"),
        CassieError::Unsupported(_)
    ));
}

#[test]
fn should_bind_null_safe_constant_defaults_with_query_coercions() {
    // Arrange
    let fixture = sql_fixture("null_safe_default_binding", &[]);
    // Act
    fixture.execute("CREATE TABLE default_coercions (id BIGINT, unknown_value BOOLEAN DEFAULT ('7' IS NOT DISTINCT FROM 7), mixed_value BOOLEAN DEFAULT (CAST(9007199254740993 AS BIGINT) IS NOT DISTINCT FROM CAST(9007199254740992 AS FLOAT)))").expect("query-bound constant defaults");
    fixture
        .execute("INSERT INTO default_coercions(id) VALUES(1)")
        .expect("apply defaults");
    let rows = fixture.rows("SELECT unknown_value,mixed_value FROM default_coercions");
    let unknown_pairs =
        fixture.rows("SELECT '7' IS NOT DISTINCT FROM 7,7 IS NOT DISTINCT FROM '7'");
    let rejected = fixture
        .execute("CREATE TABLE invalid_default (same BOOLEAN DEFAULT (TRUE IS DISTINCT FROM 1))");
    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true), Value::Bool(true)]]);
    assert_eq!(
        unknown_pairs,
        vec![vec![Value::Bool(true), Value::Bool(true)]]
    );
    let _incompatible_signature = rejected.expect_err("incompatible signatures");
    assert!(!fixture.cassie.catalog.relation_exists("invalid_default"));
}

#[test]
fn should_enforce_null_safe_checks_for_sql_null_carriers() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_check_nulls",
        &[
            "CREATE TABLE null_checks (id BIGINT,n BIGINT)",
            "INSERT INTO null_checks VALUES(1,NULL)",
            "CREATE TABLE json_checks (id BIGINT,j JSON CHECK (j IS DISTINCT FROM NULL))",
        ],
    );
    // Act
    let existing = fixture.execute(
        "ALTER TABLE null_checks ADD CONSTRAINT non_null_n CHECK (n IS DISTINCT FROM NULL)",
    );
    let missing = fixture.execute("INSERT INTO json_checks(id) VALUES(1)");
    let explicit_null = fixture.execute("INSERT INTO json_checks VALUES(2,NULL)");
    fixture
        .execute("INSERT INTO json_checks VALUES(3,'null')")
        .expect("JSON null is a value");
    // Assert
    for result in [existing, missing, explicit_null] {
        assert!(matches!(
            result.expect_err("SQL NULL violates check"),
            CassieError::CheckViolation { .. }
        ));
    }
    assert_eq!(
        fixture.rows("SELECT id FROM json_checks"),
        vec![vec![Value::Int64(3)]]
    );
}

#[test]
fn should_preserve_loaded_future_records_after_rejected_use() {
    // Arrange
    let fixture = sql_fixture(
        "future_loaded_view",
        &["CREATE VIEW future_loaded AS SELECT TRUE AS same"],
    );
    let mut metadata = fixture
        .cassie
        .midge
        .get_view("future_loaded")
        .expect("view metadata")
        .expect("view");
    metadata.definition_semantics_version = 2;
    let (key, _) = fixture
        .cassie
        .midge
        .raw_scan_prefix(cassie::midge::adapter::StorageFamily::Data, &[])
        .expect("metadata scan")
        .into_iter()
        .find(|(_, bytes)| {
            serde_json::from_slice::<cassie::catalog::ViewMeta>(bytes)
                .is_ok_and(|view| view.name == metadata.name)
        })
        .expect("actual view key");
    let bytes = serde_json::to_vec(&metadata).expect("future record");
    fixture
        .cassie
        .midge
        .raw_put(cassie::midge::adapter::StorageFamily::Data, &key, &bytes)
        .expect("unchecked future record");
    fixture.cassie.catalog.register_view(metadata);
    // Act
    let loaded = fixture.cassie.midge.get_view("future_loaded");
    let used = fixture.execute("SELECT same FROM future_loaded");
    let retained = fixture
        .cassie
        .midge
        .raw_scan_prefix(cassie::midge::adapter::StorageFamily::Data, &key)
        .expect("readback");
    // Assert
    assert!(matches!(
        loaded.expect_err("unknown loaded version"),
        CassieError::Unsupported(_)
    ));
    assert!(matches!(
        used.expect_err("unknown execution version"),
        CassieError::Unsupported(_)
    ));
    assert!(retained
        .iter()
        .any(|(stored_key, stored)| stored_key == &key && stored == &bytes));
}

#[test]
fn should_reject_future_constraint_semantics_before_alter_default_can_replace_them() {
    // Arrange
    let fixture = sql_fixture(
        "future_default_laundering",
        &["CREATE TABLE future_defaults (n BOOLEAN DEFAULT TRUE)"],
    );
    let collection =
        crate::support_sql::canonical_test_collection(&fixture.cassie, "future_defaults");
    let original = fixture
        .cassie
        .midge
        .load_constraints(&collection)
        .expect("original constraints");
    let mut future = original.clone();
    future
        .first_mut()
        .expect("default constraint")
        .definition_semantics_version = 2;
    fixture
        .cassie
        .catalog
        .register_constraints(&collection, future);
    // Act
    let rejected = fixture.execute(
        "ALTER TABLE future_defaults ALTER COLUMN n SET DEFAULT (NULL IS NOT DISTINCT FROM NULL)",
    );
    let stored = fixture
        .cassie
        .midge
        .load_constraints(&collection)
        .expect("unchanged record");
    // Assert
    assert!(matches!(
        rejected.expect_err("unknown semantics cannot be replaced"),
        CassieError::Unsupported(_)
    ));
    assert_eq!(stored, original);
}

#[test]
fn should_reject_a_failed_reader_fence_before_view_object_id_allocation() {
    // Arrange
    let fixture = sql_fixture("definition_fence_before_ids", &["SELECT 1"]);
    let schema = fixture
        .cassie
        .midge
        .raw_scan_prefix(cassie::midge::adapter::StorageFamily::Schema, &[])
        .expect("schema bootstrap");
    let key = schema
        .into_iter()
        .find_map(|(key, value)| (value == b"cassie-midge-layout-v1-0.1.0").then_some(key))
        .expect("v1 layout marker");
    fixture
        .cassie
        .midge
        .raw_put(
            cassie::midge::adapter::StorageFamily::Schema,
            &key,
            b"cassie-midge-layout-v99",
        )
        .expect("unavailable reader admission");
    let before = fixture
        .cassie
        .midge
        .raw_scan_prefix(cassie::midge::adapter::StorageFamily::Schema, &[])
        .expect("before object ID allocation");
    // Act
    let rejected =
        fixture.execute("CREATE VIEW fenced_view AS SELECT NULL IS NOT DISTINCT FROM NULL AS same");
    let after = fixture
        .cassie
        .midge
        .raw_scan_prefix(cassie::midge::adapter::StorageFamily::Schema, &[])
        .expect("after rejection");
    // Assert
    assert!(rejected.is_err());
    let object_ids = |records: Vec<(Vec<u8>, Vec<u8>)>| {
        records
            .into_iter()
            .filter(|(key, _)| {
                key.windows(b"object-id-counter".len())
                    .any(|part| part == b"object-id-counter")
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        object_ids(before),
        object_ids(after),
        "failed fence must not allocate an object ID"
    );
    assert!(!fixture.cassie.catalog.relation_exists("fenced_view"));
}

#[test]
fn should_reject_unbound_versioned_default_metadata_before_publication() {
    // Arrange
    let fixture = sql_fixture(
        "unbound_versioned_default",
        &["CREATE TABLE direct_default(n BOOLEAN)"],
    );
    let collection =
        crate::support_sql::canonical_test_collection(&fixture.cassie, "direct_default");
    let mut constraint = cassie::catalog::FieldConstraint::new("n");
    constraint.definition_semantics_version = 1;
    constraint.default_expression = Some("NULL IS NOT DISTINCT FROM NULL".into());
    let before = fixture
        .cassie
        .midge
        .load_constraints(&collection)
        .expect("original constraints");
    // Act
    let rejected = fixture
        .cassie
        .midge
        .save_constraints(&collection, &[constraint]);
    let after = fixture
        .cassie
        .midge
        .load_constraints(&collection)
        .expect("retained constraints");
    // Assert
    assert!(matches!(
        rejected.expect_err("metadata needs its bound literal carrier"),
        CassieError::Unsupported(_)
    ));
    assert_eq!(before, after);
}

#[test]
fn should_release_versioned_definition_names_after_transaction_rollback() {
    // Arrange
    let fixture = sql_fixture("null_safe_definition_rollback", &[]);
    let create = "CREATE TABLE rolled_definition (id BIGINT,n BIGINT CHECK (n IS DISTINCT FROM NULL),same BOOLEAN DEFAULT (NULL IS NOT DISTINCT FROM NULL))";
    let index =
        "CREATE INDEX rolled_predicate ON rolled_definition(n) WHERE n IS DISTINCT FROM NULL";
    fixture.execute("BEGIN").expect("transaction");
    // Act
    fixture
        .execute(create)
        .expect("staged CHECK/default definition");
    fixture.execute(index).expect("staged partial index");
    fixture.execute("ROLLBACK").expect("discard definitions");
    let absent = !fixture.cassie.catalog.relation_exists("rolled_definition");
    let indexes = fixture
        .cassie
        .midge
        .list_indexes()
        .expect("rollback readback");
    fixture
        .execute(create)
        .expect("name reusable after rollback");
    fixture
        .execute(index)
        .expect("index name reusable after rollback");
    fixture
        .execute("INSERT INTO rolled_definition(id,n) VALUES(1,7)")
        .expect("recreated default");
    // Assert
    assert!(absent);
    assert!(!indexes
        .iter()
        .any(|stored| stored.name.ends_with("rolled_predicate")));
    assert_eq!(
        fixture.rows("SELECT same FROM rolled_definition"),
        vec![vec![Value::Bool(true)]]
    );
}

#[test]
fn should_restore_previous_defaults_after_transaction_rollback() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_default_rollback",
        &["CREATE TABLE altered_default(id BIGINT,same BOOLEAN DEFAULT FALSE)"],
    );
    fixture.execute("BEGIN").expect("transaction");
    fixture.execute("ALTER TABLE altered_default ALTER COLUMN same SET DEFAULT (NULL IS NOT DISTINCT FROM NULL)").expect("staged constant default");
    // Act
    fixture.execute("ROLLBACK").expect("discard default change");
    fixture
        .execute("INSERT INTO altered_default(id) VALUES(1)")
        .expect("original default");
    let rows = fixture.rows("SELECT same FROM altered_default");
    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(false)]]);
}

#[test]
fn should_reject_versioned_view_creation_inside_transactions() {
    // Arrange
    let fixture = sql_fixture("null_safe_view_transaction_scope", &[]);
    fixture.execute("BEGIN").expect("view transaction boundary");
    let create = "CREATE VIEW transaction_view AS SELECT NULL IS NOT DISTINCT FROM NULL AS same";
    // Act
    let view = fixture.execute(create);
    fixture
        .execute("ROLLBACK")
        .expect("recover after unsupported DDL");
    fixture.execute(create).expect("view name was not reserved");
    let rows = fixture.rows("SELECT same FROM transaction_view");
    // Assert
    assert!(matches!(
        view.expect_err("existing active-transaction view exclusion"),
        CassieError::Unsupported(_)
    ));
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_maintain_versioned_null_safe_expression_indexes_after_writes() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_expression_index",
        &[
            "CREATE TABLE expression_keys(id BIGINT,n BIGINT)",
            "INSERT INTO expression_keys VALUES(1,NULL),(2,7)",
            "CREATE INDEX same_seven ON expression_keys((n IS NOT DISTINCT FROM 7))",
        ],
    );
    // Act
    fixture
        .execute("UPDATE expression_keys SET n=7 WHERE id=1")
        .expect("maintain expression keys");
    fixture
        .execute("INSERT INTO expression_keys VALUES(3,9)")
        .expect("new expression key");
    let rows =
        fixture.rows("SELECT id FROM expression_keys WHERE n IS NOT DISTINCT FROM 7 ORDER BY id");
    let indexes = fixture.cassie.midge.list_indexes().expect("indexes");
    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
    assert!(
        indexes
            .iter()
            .any(|index| index.name.ends_with("same_seven")
                && index.definition_semantics_version == 1)
    );
}
