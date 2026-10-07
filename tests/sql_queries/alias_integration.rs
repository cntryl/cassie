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

#[test]
fn should_keep_literal_table_qualifiers_distinct_from_internal_alias_carriers() {
    // Arrange
    let fixture = sql_fixture(
        "alias-literal-carrier-collision",
        &[
            "CREATE TABLE records (id INT, n BIGINT)",
            "CREATE TABLE __cassie_relation_alias_72 (id INT, n BIGINT)",
            "INSERT INTO records VALUES (11,1),(12,2)",
            "INSERT INTO __cassie_relation_alias_72 VALUES (21,1),(22,2)",
        ],
    );
    // Act
    let actual = fixture.cassie.execute_sql(&fixture.session,
        "SELECT r.id,__cassie_relation_alias_72.id FROM records r JOIN __cassie_relation_alias_72 ON r.n=__cassie_relation_alias_72.n ORDER BY r.id DESC LIMIT $1 OFFSET $2",
        vec![Value::Int64(2),Value::Int64(0)]);
    // Assert
    assert_eq!(
        actual.expect("both admitted SQL namespaces").rows,
        vec![
            vec![Value::Int64(12), Value::Int64(22)],
            vec![Value::Int64(11), Value::Int64(21)],
        ]
    );
}

#[test]
fn should_preserve_quoted_alias_ownership_and_admitted_carrier_like_names() {
    // Arrange
    let fixture = sql_fixture(
        "alias-quoted-carrier-allocation",
        &[
            "CREATE TABLE records (id INT, n BIGINT)",
            "CREATE TABLE __cassie_relation_alias_52 (id INT, n BIGINT)",
            "CREATE TABLE __cassie_bound_alias_0 (id INT, n BIGINT)",
            "INSERT INTO records VALUES (11,1),(12,2)",
            "INSERT INTO __cassie_relation_alias_52 VALUES (31,1),(32,2)",
            "INSERT INTO __cassie_bound_alias_0 VALUES (41,1),(42,2)",
        ],
    );
    let sql = r#"SELECT "R".key,__cassie_relation_alias_52.id,__cassie_bound_alias_0.id,inside.picked FROM records "R"(key,value) JOIN __cassie_relation_alias_52 ON "R".value=__cassie_relation_alias_52.n JOIN __cassie_bound_alias_0 ON "R".value=__cassie_bound_alias_0.n JOIN LATERAL (SELECT __cassie_bound_alias_1.id AS picked FROM records __cassie_bound_alias_1 WHERE __cassie_bound_alias_1.n="R".value AND EXISTS (SELECT "R".id FROM records "R" WHERE "R".id=11)) inside ON true ORDER BY "R".key DESC,inside.picked DESC LIMIT $1 OFFSET $2"#;
    // Act
    let actual = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::Int64(2), Value::Int64(0)],
    );
    let wrong_case = fixture.execute(r#"SELECT r.id FROM records "R" JOIN __cassie_relation_alias_52 ON "R".n=__cassie_relation_alias_52.n"#);
    let joined_outer_queries = [
        "SELECT records.id FROM records JOIN __cassie_relation_alias_52 ON records.n=__cassie_relation_alias_52.n WHERE EXISTS (SELECT __cassie_bound_alias_0.id FROM __cassie_bound_alias_0 WHERE __cassie_bound_alias_0.n=records.n)",
        r#"SELECT "R".key FROM records "R"(key,value) JOIN __cassie_relation_alias_52 ON "R".value=__cassie_relation_alias_52.n WHERE EXISTS (SELECT __cassie_bound_alias_0.id FROM __cassie_bound_alias_0 WHERE __cassie_bound_alias_0.n="R".value)"#,
    ];
    let joined_outer_results = joined_outer_queries.map(|sql| fixture.execute(sql));
    // Assert
    let actual = actual.expect("quoted ownership, nested shadows and carrier-like user names");
    assert_eq!(
        actual.rows,
        vec![
            vec![
                Value::Int64(12),
                Value::Int64(32),
                Value::Int64(42),
                Value::Int64(12)
            ],
            vec![
                Value::Int64(11),
                Value::Int64(31),
                Value::Int64(41),
                Value::Int64(11)
            ],
        ]
    );
    assert_eq!(actual.columns[0].name, "key");
    assert!(wrong_case
        .expect_err("quoted R does not admit lower r")
        .to_string()
        .contains("unresolvable"));
    for (sql, result) in joined_outer_queries.iter().zip(joined_outer_results) {
        assert!(result.expect_err(sql).to_string().contains("unresolvable"));
    }
}

#[test]
fn should_preserve_cte_alias_fields_when_cte_names_collide_with_private_qualifiers() {
    // Arrange
    let fixture = sql_fixture(
        "alias-cte-carrier-allocation",
        &[
            "CREATE TABLE records (id INT, n BIGINT)",
            "CREATE TABLE chosen (id INT, n BIGINT)",
            "INSERT INTO records VALUES (11,1),(12,2)",
            "INSERT INTO chosen VALUES (21,1),(22,2)",
        ],
    );
    let queries = [
        "WITH seed AS (SELECT id,n FROM records), __cassie_relation_alias_72 AS (SELECT id,n FROM chosen) SELECT r.key,__cassie_relation_alias_72.id FROM seed r(key,value) JOIN __cassie_relation_alias_72 ON r.value=__cassie_relation_alias_72.n ORDER BY r.key DESC LIMIT $1 OFFSET $2",
        r#"WITH seed AS (SELECT id,n FROM records), __cassie_relation_alias_52 AS (SELECT id,n FROM chosen) SELECT "R".key,__cassie_relation_alias_52.id FROM seed "R"(key,value) JOIN __cassie_relation_alias_52 ON "R".value=__cassie_relation_alias_52.n ORDER BY "R".key DESC LIMIT $1 OFFSET $2"#,
    ];
    // Act
    let results = queries.map(|sql| {
        fixture.cassie.execute_sql(
            &fixture.session,
            sql,
            vec![Value::Int64(2), Value::Int64(0)],
        )
    });
    // Assert
    for (sql, result) in queries.iter().zip(results) {
        let result = result.expect(sql);
        assert_eq!(
            result.rows,
            vec![
                vec![Value::Int64(12), Value::Int64(22)],
                vec![Value::Int64(11), Value::Int64(21)]
            ]
        );
        assert_eq!(result.columns[0].name, "key");
        assert_eq!(result.columns[1].name, "id");
    }
}

#[test]
fn should_reject_undeclared_fresh_private_alias_qualifier() {
    // Arrange
    let fixture = sql_fixture(
        "alias-fresh-private-reference",
        &[
            "CREATE TABLE left_records (id INT, payload BIGINT)",
            "CREATE TABLE __cassie_relation_alias_72 (id INT, payload BIGINT)",
            "INSERT INTO left_records VALUES (1,11)",
            "INSERT INTO __cassie_relation_alias_72 VALUES (1,101)",
        ],
    );
    // Act
    let actual = fixture.execute("SELECT __cassie_bound_alias_0.payload FROM left_records r JOIN __cassie_relation_alias_72 ON r.id=__cassie_relation_alias_72.id");
    // Assert
    assert!(
        actual.is_err(),
        "undeclared qualifier must not resolve: {actual:?}"
    );
}

#[test]
fn should_reject_undeclared_encoded_private_alias_qualifier() {
    // Arrange
    let fixture = sql_fixture(
        "alias-encoded-private-reference",
        &[
            "CREATE TABLE left_records (id INT, payload BIGINT)",
            "CREATE TABLE right_records (id INT, payload BIGINT)",
            "INSERT INTO left_records VALUES (1,11)",
            "INSERT INTO right_records VALUES (1,101)",
        ],
    );
    // Act
    let actual = fixture.execute("SELECT __cassie_relation_alias_72.payload FROM left_records l JOIN right_records r ON l.id=r.id");
    // Assert
    assert!(
        actual.is_err(),
        "undeclared qualifier must not resolve: {actual:?}"
    );
}

#[test]
fn should_keep_outer_alias_distinct_from_nested_physical_carrier_name() {
    // Arrange
    let fixture = sql_fixture(
        "alias-nested-physical-carrier",
        &[
            "CREATE TABLE left_records (id INT)",
            "CREATE TABLE __cassie_relation_alias_72 (id INT)",
            "INSERT INTO left_records VALUES (1)",
            "INSERT INTO __cassie_relation_alias_72 VALUES (2)",
        ],
    );
    // Act
    let actual = fixture.execute("SELECT r.id FROM left_records r WHERE EXISTS (SELECT 1 FROM __cassie_relation_alias_72 WHERE __cassie_relation_alias_72.id=r.id)");
    // Assert
    assert_eq!(
        actual.expect("existing single-source correlation").rows,
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_reject_raw_dml_inner_private_qualifiers_before_mutation() {
    // Arrange
    let fixture = sql_fixture(
        "alias-dml-private-reference",
        &[
            "CREATE TABLE left_records (id INT, payload BIGINT)",
            "CREATE TABLE right_records (id INT, payload BIGINT)",
            "CREATE TABLE target_rows (id INT PRIMARY KEY, payload BIGINT)",
            "INSERT INTO left_records VALUES (1,11)",
            "INSERT INTO right_records VALUES (1,101)",
            "INSERT INTO target_rows VALUES (1,0)",
        ],
    );
    let statements = [
        "UPDATE target_rows SET payload=7 WHERE EXISTS (SELECT 1 FROM left_records l JOIN right_records r ON l.id=r.id WHERE __cassie_relation_alias_72.payload=101)",
        "INSERT INTO target_rows VALUES (1,9) ON CONFLICT (id) DO UPDATE SET payload=9 WHERE EXISTS (SELECT 1 FROM left_records l JOIN right_records r ON l.id=r.id WHERE __cassie_relation_alias_72.payload=101)",
        "DELETE FROM target_rows WHERE EXISTS (SELECT 1 FROM left_records l JOIN right_records r ON l.id=r.id WHERE __cassie_relation_alias_72.payload=101)",
    ];
    for sql in statements {
        // Act
        let actual = fixture.execute(sql);
        let readback = fixture.execute("SELECT payload FROM target_rows");
        // Assert
        assert!(
            actual.is_err(),
            "undeclared inner qualifier must fail: {sql}: {actual:?}"
        );
        assert_eq!(
            readback.expect("unmutated source").rows,
            vec![vec![Value::Int64(0)]]
        );
    }
}

#[test]
fn should_validate_raw_alias_namespaces_in_view_definitions_and_public_binding() {
    // Arrange
    let fixture = sql_fixture(
        "alias-raw-definition-reference",
        &[
            "CREATE TABLE left_records (id INT, payload BIGINT)",
            "CREATE TABLE right_records (id INT, payload BIGINT)",
            "INSERT INTO left_records VALUES (1,11)",
            "INSERT INTO right_records VALUES (1,101)",
        ],
    );
    let invalid_view = "CREATE VIEW invalid_scope_view AS SELECT __cassie_relation_alias_72.payload FROM left_records l JOIN right_records r ON l.id=r.id";
    let initial_inputs = [
        invalid_view,
        "UPDATE left_records SET payload=7 WHERE EXISTS (SELECT 1 FROM left_records l JOIN right_records r ON l.id=r.id WHERE __cassie_relation_alias_72.payload=101)",
        "INSERT INTO left_records SELECT 1,__cassie_relation_alias_72.payload FROM left_records l JOIN right_records r ON l.id=r.id",
    ];
    // Act
    let invalid_definition = fixture.execute(invalid_view);
    let bound_inputs = initial_inputs.map(|sql| {
        cassie::sql::binder::bind(
            cassie::sql::parse_statement(sql).expect("existing syntax"),
            &fixture.cassie.catalog,
        )
    });
    let valid_definition =
        fixture.execute("CREATE VIEW valid_scope_view AS SELECT r.payload FROM left_records r");
    let valid_rows = fixture.execute("SELECT payload FROM valid_scope_view");
    // Assert
    assert!(invalid_definition.is_err());
    assert!(bound_inputs.iter().all(Result::is_err));
    valid_definition.expect("legal alias definition remains admitted");
    assert_eq!(
        valid_rows.expect("legal view read").rows,
        vec![vec![Value::Int64(11)]]
    );
}
