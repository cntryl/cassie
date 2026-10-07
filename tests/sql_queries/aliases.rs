use super::support_sql_fixture::sql_fixture;
use cassie::planner::physical::ReadAccessPath;
use cassie::runtime::ExecutionMode;
use cassie::sql::{parse_statement, QuerySource};
use cassie::types::Value;

#[test]
fn should_bind_ordinary_aliases_without_changing_underlying_columns() {
    // Arrange
    let fixture = sql_fixture(
        "ordinary_aliases",
        &[
            "CREATE TABLE public.records (id INT, score BIGINT)",
            "INSERT INTO public.records VALUES (1, 10), (2, 20), (3, 30)",
        ],
    );
    let forms = [
        "SELECT r.id FROM records AS r ORDER BY r.id",
        "SELECT r.id FROM records r ORDER BY r.id",
        "SELECT r.id FROM public.records r ORDER BY r.id",
        "SELECT \"R\".id FROM records AS \"R\" ORDER BY \"R\".id",
        "SELECT r.key FROM records r(key) ORDER BY r.key",
        "SELECT r.id FROM records r/* separator */ ORDER BY r.id",
        "WITH chosen AS (SELECT id FROM records) SELECT r.key FROM chosen r(key) ORDER BY r.key",
    ];

    // Act
    let results: Vec<_> = forms.iter().map(|sql| fixture.execute(sql)).collect();

    // Assert
    for (sql, result) in forms.iter().zip(results) {
        assert_eq!(
            result.expect(sql).rows,
            vec![
                vec![Value::Int64(1)],
                vec![Value::Int64(2)],
                vec![Value::Int64(3)],
            ]
        );
    }
}

#[test]
fn should_preserve_alias_column_prefix_and_implicit_identity_shape() {
    // Arrange
    let fixture = sql_fixture(
        "alias_identity_shape",
        &[
            "CREATE TABLE records (score BIGINT)",
            "INSERT INTO records VALUES (10), (20)",
        ],
    );
    let original = fixture
        .execute("SELECT id, score FROM records ORDER BY score")
        .expect("base");

    // Act
    let renamed = fixture.execute("SELECT * FROM records r(key) ORDER BY r.score");
    let explicit =
        fixture.execute("SELECT r.key, r.score FROM records r(key, score) ORDER BY r.score");
    let excess = fixture.execute("SELECT * FROM records r(key, score, excess)");

    // Assert
    let renamed = renamed.expect("prefix aliases");
    assert_eq!(renamed.rows, original.rows);
    assert_eq!(
        renamed
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["key", "score"]
    );
    assert_eq!(explicit.expect("complete aliases").rows, original.rows);
    assert!(excess.is_err());
}

#[test]
fn should_join_aliased_relations_and_resolve_parameterized_pagination() {
    // Arrange
    let fixture = sql_fixture(
        "alias_pagination_join",
        &[
            "CREATE TABLE records (id INT, score BIGINT)",
            "INSERT INTO records VALUES (1, 10), (2, 20), (3, 30)",
        ],
    );
    let sql = "SELECT l.key, r.score FROM records l(key) JOIN records r ON l.key=r.id ORDER BY l.key LIMIT $1 OFFSET $2";

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::Int64(1), Value::Int64(1)],
    );

    // Assert
    assert_eq!(
        result.expect("aliased join pagination").rows,
        vec![vec![Value::Int64(2), Value::Int64(20)]]
    );
    assert!(fixture.execute("SELECT records.id FROM records r").is_err());
    assert!(fixture
        .execute("SELECT r.id FROM records AS \"R\"")
        .is_err());
    assert!(fixture
        .execute("SELECT id FROM records l JOIN records r ON l.id=r.id")
        .is_err());
    assert!(fixture.execute("SELECT r.extra.id FROM records r").is_err());
}

#[test]
fn should_preserve_quoted_alias_namespaces_and_row_identity_label_roles() {
    // Arrange
    let fixture = sql_fixture(
        "alias_quoted_identity_roles",
        &[
            "CREATE TABLE records (score BIGINT)",
            "INSERT INTO records VALUES (10), (20)",
        ],
    );
    let original = fixture.rows("SELECT id, score FROM records ORDER BY score");

    // Act
    let quoted = fixture.execute("SELECT \"R\".score, r.score FROM records \"R\" JOIN records r ON \"R\".score=r.score ORDER BY \"R\".score");
    let payload_id =
        fixture.execute("SELECT r.id, r.key, r._id FROM records r(key, id) ORDER BY r.id");
    let wildcard = fixture.execute("SELECT * FROM records r(key, _id) ORDER BY r._id");

    // Assert
    assert_eq!(
        quoted.expect("case-distinct aliases").rows,
        vec![
            vec![Value::Int64(10), Value::Int64(10)],
            vec![Value::Int64(20), Value::Int64(20)]
        ]
    );
    let expected: Vec<_> = original
        .iter()
        .map(|row| vec![row[1].clone(), row[0].clone(), row[0].clone()])
        .collect();
    assert_eq!(payload_id.expect("ordinary id alias").rows, expected);
    let mut actual = wildcard.expect("ordinary _id output label").rows;
    actual.sort_by(|left, right| left[1].as_i64().cmp(&right[1].as_i64()));
    assert_eq!(actual, original);
}

#[test]
fn should_alias_wildcard_ctes_and_preserve_join_identity_hiding() {
    // Arrange
    let fixture = sql_fixture(
        "alias_cte_join_shape",
        &[
            "CREATE TABLE records (score BIGINT)",
            "CREATE TABLE declared (id BIGINT)",
            "INSERT INTO records VALUES (10), (20)",
            "INSERT INTO declared VALUES (10), (20)",
        ],
    );
    let original = fixture.rows("SELECT id, score FROM records ORDER BY score");
    let joined = fixture.execute("SELECT * FROM records JOIN declared ON records.score=declared.id ORDER BY records.score").expect("existing join");

    // Act
    let cte = fixture.execute(
        "WITH chosen AS (SELECT * FROM records) SELECT * FROM chosen r(key) ORDER BY r.score",
    );
    let aliased =
        fixture.execute("SELECT * FROM records r JOIN declared d ON r.score=d.id ORDER BY r.score");

    // Assert
    assert_eq!(cte.expect("wildcard CTE prefix").rows, original);
    let aliased = aliased.expect("aliased mixed identity join");
    assert_eq!(aliased.rows, joined.rows);
    assert_eq!(aliased.columns, joined.columns);
}

#[test]
fn should_keep_indexed_identity_pagination_and_cache_authorization_context() {
    // Arrange
    let fixture = sql_fixture(
        "alias_indexed_pagination",
        &[
            "CREATE TABLE public.records (score BIGINT NOT NULL)",
            "INSERT INTO public.records VALUES (10), (20), (30), (40)",
            "CREATE INDEX records_score_idx ON public.records USING btree (score)",
        ],
    );
    let dynamic = "SELECT r.key, r.score FROM public.records r(key) ORDER BY r.score, r._id LIMIT $1 OFFSET $2";
    let literal =
        "SELECT r.key, r.score FROM public.records r(key) ORDER BY r.score, r._id LIMIT 2";
    let original = fixture.rows("SELECT id, score FROM public.records ORDER BY score, _id");
    let first_params = vec![Value::Int64(2), Value::Int64(1)];

    // Act
    let first = fixture
        .cassie
        .execute_sql(&fixture.session, dynamic, first_params.clone());
    let second = fixture.cassie.execute_sql(
        &fixture.session,
        dynamic,
        vec![Value::Int64(1), Value::Int64(3)],
    );
    let explained = fixture.cassie.execute_sql(
        &fixture.session,
        &format!("EXPLAIN {dynamic}"),
        first_params.clone(),
    );
    let cached = fixture.execute(literal);
    let physical = fixture
        .cassie
        .compile_sql_physical_plan_for_diagnostics(literal);
    let baseline_physical = fixture.cassie.compile_sql_physical_plan_for_diagnostics(
        "SELECT id, score FROM public.records ORDER BY score, _id LIMIT 2",
    );
    let literal_cache_hit = fixture.cassie.plan_cache_hit_for_diagnostics(
        &parse_statement(literal).expect("literal AST"),
        &[],
        ExecutionMode::SimpleQuery,
        fixture.session.database.clone(),
        &fixture.session.search_path(),
    );
    let dynamic_cache_hit = fixture.cassie.plan_cache_hit_for_diagnostics(
        &parse_statement(dynamic).expect("dynamic AST"),
        &first_params,
        ExecutionMode::SimpleQuery,
        fixture.session.database.clone(),
        &fixture.session.search_path(),
    );
    let reader = super::support_alias_fixture::reader(&fixture);
    let authorized = fixture
        .cassie
        .execute_sql(&reader, dynamic, first_params.clone());
    let denied = fixture.cassie.execute_sql(
        &reader,
        "INSERT INTO public.records (score) SELECT r.score FROM public.records r LIMIT 1",
        vec![],
    );

    // Assert
    assert_eq!(first.expect("indexed first bounds").rows, original[1..3]);
    assert_eq!(second.expect("fresh bounds").rows, original[3..4]);
    let explained = explained.expect("resolved bound explain");
    let Value::String(plan) = &explained.rows[0][0] else {
        panic!("text plan");
    };
    assert!(plan.contains("access_path=ordered_bounded_scan"), "{plan}");
    assert!(plan.contains("records_score_idx"), "{plan}");
    let physical = physical.expect("physical identity");
    assert!(matches!(
        physical.logical.source,
        QuerySource::Collection(_)
    ));
    assert_eq!(
        physical.logical.collection,
        baseline_physical
            .expect("base physical identity")
            .logical
            .collection
    );
    assert!(physical.logical.collection.ends_with("public.records"));
    assert_eq!(
        physical.read.access_path,
        ReadAccessPath::OrderedBoundedScan
    );
    assert!(physical
        .read
        .projected_scan_fields
        .iter()
        .all(|field| field != "key"));
    assert_eq!(cached.expect("literal bounds").rows, original[..2]);
    assert!(literal_cache_hit);
    assert!(!dynamic_cache_hit);
    assert_eq!(
        authorized.expect("reader alias namespace").rows,
        original[1..3]
    );
    assert!(matches!(
        denied,
        Err(cassie::app::CassieError::InsufficientPrivilege)
    ));
    assert_eq!(fixture.rows("SELECT score FROM public.records").len(), 4);
}

#[test]
fn should_infer_aliased_fields_and_case_distinct_qualifiers_for_parameters() {
    // Arrange
    let fixture = sql_fixture(
        "alias_parameter_types",
        &[
            "CREATE TABLE records (id INT, score BIGINT)",
            "CREATE TABLE labels (score TEXT)",
        ],
    );
    let prefix = cassie::sql::parse_statement(
        "SELECT r.key FROM records r(key) WHERE r.key=$1 LIMIT $2 OFFSET $3",
    )
    .expect("prefix parameters");
    let quoted = cassie::sql::parse_statement(
        "SELECT \"R\".id FROM records \"R\" JOIN labels r ON \"R\".score=$1 AND r.score=$2 LIMIT $3").expect("case-distinct parameters");

    // Act
    let prefix_oids =
        cassie::sql::parameter_type_oids_with_catalog(&prefix, &[], &fixture.cassie.catalog);
    let quoted_oids =
        cassie::sql::parameter_type_oids_with_catalog(&quoted, &[], &fixture.cassie.catalog);

    // Assert
    assert_eq!(prefix_oids, vec![23, 20, 20]);
    assert_eq!(quoted_oids, vec![20, 25, 20]);
}

#[test]
fn should_hide_original_unqualified_names_after_prefix_renaming() {
    // Arrange
    let fixture = sql_fixture(
        "alias_hidden_fields",
        &[
            "CREATE TABLE records (id INT, score BIGINT)",
            "INSERT INTO records VALUES (1, 20), (2, 10)",
        ],
    );

    // Act
    let old_id = fixture.execute("SELECT id FROM records r(key, amount)");
    let old_score = fixture.execute("SELECT score FROM records r(key, amount)");
    let visible = fixture.execute("SELECT key, amount FROM records r(key, amount) ORDER BY key");

    // Assert
    assert!(old_id.is_err());
    assert!(old_score.is_err());
    assert_eq!(
        visible.expect("visible renamed fields").rows,
        vec![
            vec![Value::Int64(1), Value::Int64(20)],
            vec![Value::Int64(2), Value::Int64(10)]
        ]
    );
}

#[test]
fn should_order_by_explicit_output_alias_before_the_source_alias_column() {
    // Arrange
    let fixture = sql_fixture(
        "alias_order_precedence",
        &[
            "CREATE TABLE records (id INT, score BIGINT)",
            "INSERT INTO records VALUES (1, 20), (2, 10)",
        ],
    );

    // Act
    let rows =
        fixture.execute("SELECT r.key AS amount FROM records r(key, amount) ORDER BY amount");
    let expressions =
        fixture.execute("SELECT r.key+1 AS amount FROM records r(key, amount) ORDER BY amount");

    let source_order =
        fixture.execute("SELECT r.key AS score FROM records r(key,amount) ORDER BY r.amount");

    // Assert
    assert_eq!(
        rows.expect("output alias order").rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    assert_eq!(
        expressions.expect("expression output alias order").rows,
        vec![vec![Value::Int64(2)], vec![Value::Int64(3)]]
    );
    assert_eq!(
        source_order
            .expect("qualified source precedes colliding output label")
            .rows,
        vec![vec![Value::Int64(2)], vec![Value::Int64(1)]]
    );
}

#[test]
fn should_reject_ambiguous_duplicate_source_column_aliases() {
    // Arrange
    let fixture = sql_fixture(
        "alias_duplicate_columns",
        &["CREATE TABLE records (id INT, score BIGINT)"],
    );

    // Act
    let qualified = fixture.execute("SELECT r.same FROM records r(same, same)");
    let unqualified = fixture.execute("SELECT same FROM records r(same, same)");
    let wildcard = fixture.execute("SELECT * FROM records r(same, same)");

    // Assert
    assert!(qualified.is_err());
    assert!(unqualified.is_err());
    assert_eq!(
        wildcard
            .expect("duplicate output labels remain valid")
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["same", "same"]
    );
}

#[test]
fn should_preserve_alias_scopes_in_existing_correlated_selects() {
    // Arrange
    let fixture = sql_fixture(
        "alias_correlated_select",
        &[
            "CREATE TABLE records (id INT)",
            "CREATE TABLE chosen (id INT)",
            "INSERT INTO records VALUES (1), (2)",
            "INSERT INTO chosen VALUES (2)",
        ],
    );
    let control = "SELECT id FROM records WHERE EXISTS (SELECT id FROM chosen WHERE chosen.id=records.id LIMIT $1) ORDER BY id";
    let candidates = [
        "SELECT id FROM records WHERE EXISTS (SELECT c.key FROM chosen c(key) WHERE c.key=records.id LIMIT $1) ORDER BY id",
        "SELECT r.key FROM records r(key) WHERE EXISTS (SELECT id FROM chosen WHERE chosen.id=r.key LIMIT $1) ORDER BY r.key",
        "SELECT r.key FROM records r(key) WHERE EXISTS (SELECT c.key FROM chosen c(key) WHERE c.key=r.key LIMIT $1) ORDER BY r.key",
        "SELECT \"R\".key FROM records \"R\"(key) WHERE EXISTS (SELECT r.key FROM chosen r(key) WHERE r.key=\"R\".key LIMIT $1) ORDER BY \"R\".key",
    ];

    // Act
    let control = fixture
        .cassie
        .execute_sql(&fixture.session, control, vec![Value::Int64(1)]);
    let results: Vec<_> = candidates
        .iter()
        .map(|sql| {
            fixture
                .cassie
                .execute_sql(&fixture.session, sql, vec![Value::Int64(1)])
        })
        .collect();

    let shadowed = fixture.cassie.execute_sql(&fixture.session,
        "SELECT r.key FROM records r(key) WHERE EXISTS (SELECT r.key FROM chosen r(key) WHERE r.key=2 LIMIT $1) ORDER BY r.key", vec![Value::Int64(1)]);
    let unsupported_on = fixture.cassie.execute_sql(&fixture.session,
        "SELECT id FROM records WHERE EXISTS (SELECT chosen.id FROM chosen JOIN (SELECT id FROM chosen) inside ON chosen.id=records.id AND inside.id=chosen.id LIMIT $1) ORDER BY id", vec![Value::Int64(1)]);
    let aliased_on = fixture.cassie.execute_sql(&fixture.session,
        "SELECT r.key FROM records r(key) WHERE EXISTS (SELECT chosen.id FROM chosen JOIN records ON chosen.id=r.key AND records.id=chosen.id LIMIT $1) ORDER BY r.key", vec![Value::Int64(1)]);

    // Assert
    assert_eq!(
        shadowed.expect("inner alias shadows outer alias").rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    assert!(unsupported_on
        .expect_err("existing outer reference in JOIN ON is unsupported")
        .to_string()
        .contains("unresolvable column reference"));
    assert!(aliased_on
        .expect_err("aliases preserve the existing JOIN ON boundary")
        .to_string()
        .contains("unresolvable column reference"));
    assert_eq!(
        control.expect("existing correlated path").rows,
        vec![vec![Value::Int64(2)]]
    );
    for (sql, result) in candidates.iter().zip(results) {
        assert_eq!(result.expect(sql).rows, vec![vec![Value::Int64(2)]]);
    }
}

#[test]
fn should_preserve_ordinary_aliases_in_existing_lateral_scopes() {
    // Arrange
    let fixture = sql_fixture(
        "alias_lateral_scope",
        &[
            "CREATE TABLE records (id INT)",
            "CREATE TABLE chosen (id INT, score BIGINT)",
            "INSERT INTO records VALUES (1), (2)",
            "INSERT INTO chosen VALUES (1, 10), (1, 20), (2, 30)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT r.key, recent.score FROM records r(key) JOIN LATERAL (SELECT score FROM chosen WHERE chosen.id=r.key ORDER BY score DESC LIMIT 1) recent ON true ORDER BY r.key");

    // Assert
    assert_eq!(
        result.expect("existing lateral alias scope").rows,
        vec![
            vec![Value::Int64(1), Value::Int64(20)],
            vec![Value::Int64(2), Value::Int64(30)]
        ]
    );
}
