use crate::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;

#[test]
fn should_reject_loaded_materialized_projection_rebuild_definition() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_loaded_projection",
        &[
            "CREATE TABLE projection_source (n BIGINT)",
            "INSERT INTO projection_source VALUES(7)",
            "CREATE MATERIALIZED PROJECTION loaded_projection AS SELECT n FROM projection_source",
        ],
    );
    let mut metadata = fixture
        .cassie
        .catalog
        .get_materialized_projection("loaded_projection")
        .expect("existing materialized definition");
    metadata.materialized.as_mut().expect("definition").query =
        "SELECT n FROM projection_source WHERE n IS DISTINCT FROM NULL".into();
    let prior_versions =
        serde_json::to_value(&metadata.versions).expect("existing version representation");
    fixture
        .cassie
        .catalog
        .register_projection_metadata(metadata);
    // Act
    let result = fixture.execute("ALTER MATERIALIZED PROJECTION loaded_projection BUILD VERSION");
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "{result:?}"
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .cassie
                .catalog
                .get_materialized_projection("loaded_projection")
                .expect("retained definition")
                .versions
        )
        .expect("unchanged version representation"),
        prior_versions
    );
}

#[test]
fn should_reject_loaded_rollup_refresh_definition() {
    // Arrange
    let fixture = sql_fixture("null_safe_loaded_rollup", &[
        "CREATE TABLE rollup_source (tenant TEXT,event_at TEXT,n BIGINT)",
        "INSERT INTO rollup_source VALUES('a','2026-01-01T00:05:00Z',7)",
        "CREATE ROLLUP loaded_rollup ON rollup_source USING time_bucket('1 hour',event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total",
    ]);
    let mut metadata = fixture
        .cassie
        .catalog
        .list_rollups()
        .into_iter()
        .next()
        .expect("existing rollup");
    metadata.filter_expr = Some("n IS DISTINCT FROM NULL".into());
    let prior = metadata.clone();
    fixture.cassie.catalog.register_rollup(metadata);
    // Act
    let result = fixture.execute("REFRESH ROLLUP loaded_rollup");
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "{result:?}"
    );
    assert_eq!(
        fixture
            .cassie
            .catalog
            .get_rollup(&prior.name)
            .expect("unchanged metadata"),
        prior
    );
}

#[test]
fn should_reject_unchecked_stored_index_predicate_maintenance() {
    // Arrange
    let fixture = sql_fixture(
        "null_safe_loaded_index_write",
        &[
            "CREATE TABLE index_write_source(id BIGINT,n BIGINT)",
            "CREATE INDEX loaded_write_n ON index_write_source(n)",
        ],
    );
    let mut index = fixture
        .cassie
        .midge
        .list_indexes()
        .expect("metadata list")
        .into_iter()
        .find(|index| index.kind == cassie::catalog::IndexKind::Scalar)
        .expect("existing scalar index");
    let parsed = cassie::sql::parse_statement("SELECT n IS NOT DISTINCT FROM NULL AS same")
        .expect("actual predicate AST");
    let cassie::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT AST")
    };
    let cassie::sql::ast::SelectItem::Expr { expr, .. } =
        select.projection.into_iter().next().expect("one predicate")
    else {
        panic!("predicate Expr")
    };
    index.predicate = Some(serde_json::to_string(&expr).expect("existing encoded Expr"));
    let (key, _) = fixture
        .cassie
        .midge
        .raw_scan_prefix(cassie::midge::adapter::StorageFamily::Schema, &[])
        .expect("actual keys")
        .into_iter()
        .find(|(_, bytes)| {
            serde_json::from_slice::<cassie::catalog::IndexMeta>(bytes)
                .is_ok_and(|stored| stored.name == index.name)
        })
        .expect("actual index key");
    fixture
        .cassie
        .midge
        .raw_put(
            cassie::midge::adapter::StorageFamily::Schema,
            &key,
            &serde_json::to_vec(&index).expect("existing record"),
        )
        .expect("unchecked metadata setup");
    let collection = index.collection.clone();
    fixture.cassie.catalog.register_index(index);
    // Act
    let result = fixture.execute("INSERT INTO index_write_source VALUES(1,7)");
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "{result:?}"
    );
    assert!(fixture
        .cassie
        .midge
        .scan_documents(&collection)
        .expect("unchanged authoritative data")
        .is_empty());
}

#[test]
fn should_reject_loaded_materialized_projection_repair_definition() {
    // Arrange
    let fixture = sql_fixture("null_safe_loaded_repair", &[]);
    fixture
        .cassie
        .startup()
        .expect("initialize canonical database scope");
    for sql in [
        "CREATE TABLE repair_source(n BIGINT)",
        "INSERT INTO repair_source VALUES(7)",
        "CREATE MATERIALIZED PROJECTION loaded_repair AS SELECT n FROM repair_source",
        "ALTER MATERIALIZED PROJECTION loaded_repair BUILD VERSION",
    ] {
        fixture.execute(sql).expect("supported version fixture");
    }
    let metadata = fixture
        .cassie
        .catalog
        .get_materialized_projection("loaded_repair")
        .expect("definition");
    let version = metadata.versions.last().expect("built version").clone();
    let expected = fixture
        .cassie
        .midge
        .list_row_hashes(&version.output_collection)
        .expect("canonical version hashes")
        .into_iter()
        .next()
        .expect("built row hash");
    let (key, mut hash) = fixture
        .cassie
        .midge
        .raw_scan_prefix(cassie::midge::adapter::StorageFamily::Data, &[])
        .expect("actual data keys")
        .into_iter()
        .find_map(|(key, bytes)| {
            let record =
                serde_json::from_slice::<cassie::midge::adapter::RowHashRecord>(&bytes).ok()?;
            (record.collection == expected.collection && record.row_id == expected.row_id)
                .then_some((key, record))
        })
        .expect("exact authoritative version row-hash key");
    hash.state = cassie::midge::adapter::StoredHashState::Stale;
    fixture
        .cassie
        .midge
        .raw_put(
            cassie::midge::adapter::StorageFamily::Data,
            &key,
            &serde_json::to_vec(&hash).expect("record"),
        )
        .expect("controlled stale hash");
    fixture
        .execute(&format!(
            "VERIFY PROJECTION loaded_repair VERSION {} MODE full",
            version.version_id
        ))
        .expect("actual prior verification");
    let mut metadata = fixture
        .cassie
        .catalog
        .get_materialized_projection("loaded_repair")
        .expect("verified definition");
    assert!(metadata.integrity.repairable);
    metadata.materialized.as_mut().expect("materialized").query =
        "SELECT n FROM repair_source WHERE n IS DISTINCT FROM NULL".into();
    let prior = serde_json::to_value(&metadata).expect("original metadata");
    fixture
        .cassie
        .catalog
        .register_projection_metadata(metadata);
    // Act
    let result = fixture.execute(&format!(
        "REPAIR PROJECTION loaded_repair VERSION {} SCOPE projection-version",
        version.version_id
    ));
    // Assert
    assert!(
        matches!(result, Err(CassieError::Unsupported(ref message)) if message.contains("persisted definitions")),
        "{result:?}"
    );
    assert_eq!(
        serde_json::to_value(
            fixture
                .cassie
                .catalog
                .get_materialized_projection("loaded_repair")
                .expect("retained metadata")
        )
        .expect("unchanged metadata"),
        prior
    );
}
