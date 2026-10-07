//! Boundaries shared by approved aliases, pagination and prepared scalar/relational slices.
use super::support_sql_fixture::sql_fixture;
use super::support_typed_join::JoinFixture;
use cassie::types::Value;

#[test]
fn should_preserve_scalar_origin_and_single_conditional_normalization_with_alias_bounds() {
    // Arrange
    let fixture = sql_fixture(
        "alias-scalar-integration",
        &[
            "CREATE TABLE records (n BIGINT, f FLOAT)",
            "INSERT INTO records VALUES (9007199254740993, NULL)",
        ],
    );
    let queries = [
        "SELECT COALESCE(r.n, r.f)-1 FROM records r(key,n,f) LIMIT $1",
        "WITH first_scope AS (SELECT COALESCE(r.n,r.f) AS v FROM records r(key,n,f)), second_scope AS (SELECT GREATEST(c.value,0.0) AS v FROM first_scope c(value)) SELECT v-1 FROM second_scope LIMIT $1",
    ];

    // Act
    let results = queries.map(|sql| {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::Int64(1)])
    });
    let nonfinite = fixture.cassie.execute_sql(&fixture.session,
        "WITH first_scope AS (SELECT GREATEST(1e308*10,0) AS value), second_scope AS (SELECT LEAST(c.v,c.v) AS value FROM first_scope c(v)) SELECT NULLIF(r.v,0) FROM second_scope r(v) LIMIT $1", vec![Value::Int64(1)]);
    let explicit_cast = fixture
        .execute("SELECT COALESCE(CAST(r.n*1e308 AS FLOAT),r.f)-1 FROM records r(key,n,f) LIMIT 1");

    // Assert
    for (sql, result) in queries.iter().zip(results) {
        let result = result.expect(sql);
        assert_eq!(
            result.rows,
            vec![vec![Value::Float64(9_007_199_254_740_991.0)]]
        );
        assert_eq!(result.columns[0].data_type, "float");
    }
    assert_eq!(
        nonfinite
            .expect("one complete-bound normalization pass")
            .rows,
        vec![vec![Value::Float64(f64::INFINITY)]]
    );
    assert!(explicit_cast
        .expect_err("explicit SQL casts keep their error boundary")
        .to_string()
        .contains("cannot cast value to FLOAT"));
}

#[test]
fn should_preserve_typed_join_collection_identity_with_evaluated_alias_bounds() {
    // Arrange
    let setup = [
        "CREATE TABLE left_records (n BIGINT, tag TEXT)",
        "CREATE TABLE right_records (n FLOAT, tag TEXT)",
        "INSERT INTO left_records VALUES (0,'zero'),(1,'one'),(1,'again'),(NULL,'null')",
        "INSERT INTO right_records VALUES (-0.0,'negative-zero'),(1.0,'first'),(1.0,'second'),(NULL,'never')",
    ];
    let native = JoinFixture::new(true, &setup);
    let scalar = JoinFixture::new(false, &setup);
    let alias_sql = "SELECT l.label AS tag,r.label AS tag FROM left_records l(key,value,label) JOIN right_records r(key,value,label) ON l.value=r.value ORDER BY l.label,r.label LIMIT $1 OFFSET $2";
    let base_sql = "SELECT left_records.tag,right_records.tag FROM left_records JOIN right_records ON left_records.n=right_records.n ORDER BY left_records.tag,right_records.tag LIMIT $1 OFFSET $2";

    // Act
    let actual = native.execute_with_params(alias_sql, vec![Value::Int64(3), Value::Int64(1)]);
    let alias_strategy = native.cassie.metrics()["joins"]["last_strategy"].clone();
    let expected = scalar.execute_with_params(alias_sql, vec![Value::Int64(3), Value::Int64(1)]);
    let ordinary_sql = "SELECT l.tag,r.tag FROM left_records l JOIN right_records r ON l.n=r.n ORDER BY l.tag,r.tag LIMIT $1 OFFSET $2";
    let ordinary = native.execute_with_params(ordinary_sql, vec![Value::Int64(3), Value::Int64(1)]);
    let ordinary_strategy = native.cassie.metrics()["joins"]["last_strategy"].clone();
    let base = native.execute_with_params(base_sql, vec![Value::Int64(3), Value::Int64(1)]);

    // Assert
    assert_eq!(actual.columns, expected.columns);
    assert_eq!(actual.rows, expected.rows);
    assert_eq!(actual.rows, base.rows);
    assert_eq!(ordinary.rows, base.rows);
    assert_eq!(ordinary_strategy, "typed_hash");
    assert_eq!(actual.rows.len(), 3);
    assert_eq!(alias_strategy, "typed_hash");
    assert_eq!(
        native.cassie.metrics()["joins"]["last_strategy"],
        "typed_hash"
    );
    assert_eq!(
        native.cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_preserve_indexed_reads_when_qualified_order_conflicts_with_an_output_label() {
    // Arrange
    use cassie::planner::physical::ReadAccessPath;
    let fixture = sql_fixture(
        "alias-order-index-identity",
        &[
            "CREATE TABLE records (id INT, score BIGINT NOT NULL)",
            "INSERT INTO records VALUES (1,20),(2,10),(3,30)",
            "CREATE INDEX records_score_idx ON records USING btree (score)",
        ],
    );
    let base_sql = "SELECT id AS key FROM records ORDER BY score LIMIT 2";
    let alias_sql = "SELECT r.key AS score FROM records r(key,amount) ORDER BY r.amount LIMIT 2";
    let base_plan = fixture
        .cassie
        .compile_sql_physical_plan_for_diagnostics(base_sql);
    let alias_plan = fixture
        .cassie
        .compile_sql_physical_plan_for_diagnostics(alias_sql);
    let base = fixture.rows(base_sql);
    let before = fixture.cassie.metrics()["read_paths"]["ordered_bounded_scans"]
        .as_u64()
        .expect("counter");

    // Act
    let actual = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT r.key AS score FROM records r(key,amount) ORDER BY r.amount LIMIT $1 OFFSET $2",
        vec![Value::Int64(2), Value::Int64(0)],
    );
    let after = fixture.cassie.metrics()["read_paths"]["ordered_bounded_scans"]
        .as_u64()
        .expect("counter");

    // Assert
    assert_eq!(actual.expect("alias namespace and accelerator").rows, base);
    assert_eq!(
        base_plan.expect("base plan").read.access_path,
        ReadAccessPath::OrderedBoundedScan
    );
    assert_eq!(
        alias_plan.expect("alias plan").read.access_path,
        ReadAccessPath::OrderedBoundedScan
    );
    assert_eq!(after, before + 1);
}

#[test]
fn should_preserve_quoted_physical_fields_in_alias_indexed_projection() {
    // Arrange
    use cassie::planner::physical::ReadAccessPath;
    let fixture = sql_fixture(
        "alias-quoted-index-identity",
        &[
            r#"CREATE TABLE records (id INT, "Amount Value" BIGINT NOT NULL)"#,
            "INSERT INTO records VALUES (1,20),(2,10),(3,30)",
            r#"CREATE INDEX records_amount_idx ON records USING btree ("Amount Value")"#,
        ],
    );
    let base = fixture.rows(r#"SELECT id AS key, "Amount Value" AS actual FROM records ORDER BY "Amount Value" LIMIT 2"#);
    let sql = r#"SELECT r.key AS "Amount Value", r."Amount" AS actual FROM records r(key,"Amount") ORDER BY r."Amount" LIMIT $1 OFFSET $2"#;
    let diagnostic = fixture.cassie.compile_sql_physical_plan_for_diagnostics(
        r#"SELECT r.key AS "Amount Value", r."Amount" AS actual FROM records r(key,"Amount") ORDER BY r."Amount" LIMIT 2"#);
    let before = fixture.cassie.metrics()["read_paths"]["ordered_bounded_scans"]
        .as_u64()
        .expect("counter");
    // Act
    let actual = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            sql,
            vec![Value::Int64(2), Value::Int64(0)],
        )
        .expect("exact physical field handoff");
    // Assert
    assert_eq!(actual.rows, base);
    assert_eq!(
        actual.rows,
        vec![
            vec![Value::Int64(2), Value::Int64(10)],
            vec![Value::Int64(1), Value::Int64(20)]
        ]
    );
    assert_eq!(actual.columns[0].name, "Amount Value");
    assert_eq!(actual.columns[1].name, "actual");
    assert_eq!(actual.columns[1].data_type, "bigint");
    assert_eq!(
        diagnostic.expect("plan").read.access_path,
        ReadAccessPath::OrderedBoundedScan
    );
    assert_eq!(
        fixture.cassie.metrics()["read_paths"]["ordered_bounded_scans"]
            .as_u64()
            .expect("counter"),
        before + 1
    );
}
