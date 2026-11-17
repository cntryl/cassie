//! View dependencies follow graph identity within the owning database.

use crate::graph_view_fixture::started_shadow_fixture;
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE TABLE v(id INT PRIMARY KEY)",
    "CREATE TABLE e(id INT PRIMARY KEY,a INT,b INT)",
    "INSERT INTO v VALUES(1)",
    "CREATE PROPERTY GRAPH g VERTEX TABLES(v AS n KEY(id) LABEL node PROPERTIES(id)) \
     EDGE TABLES(e AS l KEY(id) SOURCE KEY(a) REFERENCES n(id) \
     DESTINATION KEY(b) REFERENCES n(id) LABEL edge PROPERTIES(id))",
];
const VIEW: &str =
    "CREATE VIEW vg AS SELECT x FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.id AS x)) AS t";

#[test]
fn should_reject_a_graph_rename_while_its_view_depends_on_the_old_name() {
    // Arrange
    let fixture = sql_fixture("graph-view-rename-dependency", SETUP);
    fixture.execute(VIEW).expect("create dependent view");
    assert_eq!(
        fixture.rows("SELECT x FROM vg"),
        vec![vec![Value::Int64(1)]]
    );

    // Act
    let renamed = fixture.execute("ALTER PROPERTY GRAPH g RENAME TO renamed");

    // Assert
    assert_eq!(
        renamed
            .expect_err("dependent view blocks rename")
            .sql_state(),
        "2BP01"
    );
    assert_eq!(
        fixture.rows("SELECT x FROM vg"),
        vec![vec![Value::Int64(1)]]
    );
    assert_eq!(
        fixture.rows("SELECT * FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.id)) AS t"),
        vec![vec![Value::Int64(1)]]
    );
    assert!(fixture
        .execute("SELECT * FROM GRAPH_TABLE(renamed MATCH(v IS node) COLUMNS(v.id)) AS t")
        .is_err());
}

#[test]
fn should_drop_the_transitive_graph_view_closure_with_its_schema_and_restore_it_on_rollback() {
    // Arrange
    let fixture = sql_fixture("graph-view-schema-cascade", &[
        "CREATE TABLE v(id INT PRIMARY KEY)",
        "CREATE TABLE e(id INT PRIMARY KEY,a INT,b INT)",
        "INSERT INTO v VALUES(1)",
        "CREATE SCHEMA graph_schema",
        "CREATE PROPERTY GRAPH graph_schema.g VERTEX TABLES(v AS n KEY(id) LABEL node PROPERTIES(id)) \
         EDGE TABLES(e AS l KEY(id) SOURCE KEY(a) REFERENCES n(id) \
         DESTINATION KEY(b) REFERENCES n(id) LABEL edge PROPERTIES(id))",
        "CREATE VIEW vg AS SELECT x FROM GRAPH_TABLE(graph_schema.g MATCH(v IS node) COLUMNS(v.id AS x)) AS t",
        "CREATE VIEW vg_outer AS SELECT x FROM vg",
        "CREATE VIEW unrelated AS SELECT 7 AS x",
    ]);
    fixture.execute("BEGIN").expect("begin cascade transaction");

    // Act
    fixture
        .execute("DROP SCHEMA graph_schema CASCADE")
        .expect("cascade graph schema");
    let surviving = fixture
        .rows("SELECT relname FROM pg_class WHERE relname IN ('vg','vg_outer') ORDER BY relname");
    let unrelated = fixture.rows("SELECT x FROM unrelated");
    fixture
        .execute("ROLLBACK")
        .expect("restore graph and view closure");
    let restored = fixture.rows("SELECT x FROM vg_outer");

    // Assert
    assert!(
        surviving.is_empty(),
        "direct and transitive external views must be removed"
    );
    assert_eq!(unrelated, vec![vec![Value::Int64(7)]]);
    assert_eq!(restored, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_ignore_other_databases_when_checking_graph_view_dependencies() {
    // Arrange
    let fixture = sql_fixture("graph-view-database-dependency", &[]);
    fixture
        .cassie
        .startup()
        .expect("bootstrap database scoping");
    let original = fixture.cassie.create_session("tester", None);
    for sql in SETUP {
        fixture
            .cassie
            .execute_sql(&original, sql, vec![])
            .expect("seed original graph");
    }
    fixture
        .cassie
        .execute_sql(&original, VIEW, vec![])
        .expect("create dependent view");
    let before = fixture
        .cassie
        .execute_sql(&original, "SELECT x FROM vg", vec![])
        .expect("view is initially usable");
    assert_eq!(before.rows(), &[vec![Value::Int64(1)]]);
    fixture
        .cassie
        .execute_sql(&original, "CREATE DATABASE other_graph_db", vec![])
        .expect("create independent database");
    let other = fixture
        .cassie
        .create_session("tester", Some("other_graph_db".to_owned()));
    for sql in SETUP {
        fixture
            .cassie
            .execute_sql(&other, sql, vec![])
            .expect("seed independent graph");
    }

    // Act
    let dropped = fixture
        .cassie
        .execute_sql(&other, "DROP PROPERTY GRAPH g", vec![]);
    let original_view = fixture
        .cassie
        .execute_sql(&original, "SELECT x FROM vg", vec![])
        .expect("original view remains usable");

    // Assert
    assert!(
        dropped.is_ok(),
        "a foreign database view cannot depend on this graph: {dropped:?}"
    );
    assert_eq!(original_view.rows(), &[vec![Value::Int64(1)]]);
}

const SHADOW_SETUP: &[&str] = &[
    "CREATE SCHEMA common",
    "CREATE SCHEMA alt",
    "CREATE SCHEMA keep",
    "CREATE TABLE common.v(id INT PRIMARY KEY)",
    "CREATE TABLE common.e(id INT PRIMARY KEY,a INT,b INT)",
    "INSERT INTO common.v VALUES(2)",
    "CREATE PROPERTY GRAPH public.g VERTEX TABLES(common.v AS n KEY(id) LABEL node PROPERTIES(id)) \
     EDGE TABLES(common.e AS l KEY(id) SOURCE KEY(a) REFERENCES n(id) \
     DESTINATION KEY(b) REFERENCES n(id) LABEL edge PROPERTIES(id))",
    "CREATE PROPERTY GRAPH alt.g VERTEX TABLES(common.v AS n KEY(id) LABEL node PROPERTIES(id)) \
     EDGE TABLES(common.e AS l KEY(id) SOURCE KEY(a) REFERENCES n(id) \
     DESTINATION KEY(b) REFERENCES n(id) LABEL edge PROPERTIES(id))",
];

#[test]
fn should_protect_every_potential_graph_target_of_a_shadowed_unqualified_view_reference() {
    // Arrange
    let fixture = started_shadow_fixture("graph-view-shadowed-drop", SHADOW_SETUP);
    fixture.execute("CREATE VIEW alt.vg AS SELECT x FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.id AS x)) AS t").expect("unqualified view binds public graph");
    assert_eq!(
        fixture.rows("SELECT x FROM alt.vg"),
        vec![vec![Value::Int64(2)]]
    );

    // Act
    let dropped = fixture.execute("DROP PROPERTY GRAPH public.g");
    let renamed = fixture.execute("ALTER PROPERTY GRAPH alt.g RENAME TO renamed");

    // Assert
    assert_eq!(
        dropped
            .expect_err("potential graph target blocks drop")
            .sql_state(),
        "2BP01"
    );
    assert_eq!(
        renamed
            .expect_err("potential graph target blocks rename")
            .sql_state(),
        "2BP01"
    );
    assert_eq!(
        fixture.rows("SELECT x FROM alt.vg"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT * FROM GRAPH_TABLE(alt.g MATCH(v IS node) COLUMNS(v.id)) AS t"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_reject_renaming_a_potential_graph_target_of_a_shadowed_view_reference() {
    // Arrange
    let fixture = started_shadow_fixture("graph-view-shadowed-rename", SHADOW_SETUP);
    fixture.execute("CREATE VIEW alt.vg AS SELECT x FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.id AS x)) AS t").expect("unqualified view binds public graph");
    assert_eq!(
        fixture.rows("SELECT x FROM alt.vg"),
        vec![vec![Value::Int64(2)]]
    );

    // Act
    let renamed = fixture.execute("ALTER PROPERTY GRAPH public.g RENAME TO renamed");

    // Assert
    assert_eq!(
        renamed
            .expect_err("potential graph target blocks rename")
            .sql_state(),
        "2BP01"
    );
    assert_eq!(
        fixture.rows("SELECT x FROM alt.vg"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT * FROM GRAPH_TABLE(public.g MATCH(v IS node) COLUMNS(v.id)) AS t"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_reject_an_ambiguous_direct_graph_cascade_before_mutating_its_schema() {
    // Arrange
    let fixture = started_shadow_fixture("graph-view-shadowed-direct-cascade", SHADOW_SETUP);
    fixture.execute("CREATE VIEW keep.vg AS SELECT x FROM GRAPH_TABLE(g MATCH(v IS node) COLUMNS(v.id AS x)) AS t").expect("unqualified outside view");
    assert_eq!(
        fixture.rows("SELECT x FROM keep.vg"),
        vec![vec![Value::Int64(2)]]
    );

    // Act
    let dropped = fixture.execute("DROP SCHEMA alt CASCADE");

    // Assert
    assert_eq!(
        dropped
            .expect_err("ambiguous graph target refuses cascade")
            .sql_state(),
        "0A000"
    );
    assert_eq!(
        fixture.rows("SELECT * FROM GRAPH_TABLE(alt.g MATCH(v IS node) COLUMNS(v.id)) AS t"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT * FROM GRAPH_TABLE(public.g MATCH(v IS node) COLUMNS(v.id)) AS t"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT x FROM keep.vg"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_reject_a_transitive_cascade_with_a_surviving_shadow_relation() {
    // Arrange
    let fixture = started_shadow_fixture("graph-view-shadowed-transitive-cascade", SHADOW_SETUP);
    fixture.execute("CREATE VIEW public.base AS SELECT x FROM GRAPH_TABLE(alt.g MATCH(v IS node) COLUMNS(v.id AS x)) AS t").expect("qualified direct view");
    fixture
        .execute("CREATE TABLE keep.base(x INT)")
        .expect("surviving table shadows the view");
    fixture
        .execute("INSERT INTO keep.base VALUES(99)")
        .expect("shadow table row");
    fixture
        .execute("CREATE VIEW keep.outer_view AS SELECT x FROM base")
        .expect("unqualified view of view");
    assert_eq!(
        fixture.rows("SELECT x FROM keep.outer_view"),
        vec![vec![Value::Int64(2)]]
    );

    // Act
    let dropped = fixture.execute("DROP SCHEMA alt CASCADE");

    // Assert
    assert_eq!(
        dropped
            .expect_err("ambiguous relation target refuses cascade")
            .sql_state(),
        "0A000"
    );
    assert_eq!(
        fixture.rows("SELECT * FROM GRAPH_TABLE(alt.g MATCH(v IS node) COLUMNS(v.id)) AS t"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT x FROM public.base"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT x FROM keep.outer_view"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT x FROM keep.base"),
        vec![vec![Value::Int64(99)]]
    );
}

#[test]
fn should_reject_a_transitive_cascade_with_a_surviving_shadow_view() {
    // Arrange
    let fixture = started_shadow_fixture("graph-view-shadowed-transitive-view", SHADOW_SETUP);
    fixture.execute("CREATE VIEW public.base AS SELECT x FROM GRAPH_TABLE(alt.g MATCH(v IS node) COLUMNS(v.id AS x)) AS t").expect("qualified direct view");
    fixture
        .execute("CREATE VIEW keep.base AS SELECT 99 AS x")
        .expect("surviving view shadows the dependent view");
    fixture
        .execute("CREATE VIEW keep.outer_view AS SELECT x FROM base")
        .expect("unqualified view of view");
    assert_eq!(
        fixture.rows("SELECT x FROM keep.outer_view"),
        vec![vec![Value::Int64(2)]]
    );

    // Act
    let dropped = fixture.execute("DROP SCHEMA alt CASCADE");

    // Assert
    assert_eq!(
        dropped
            .expect_err("ambiguous view target refuses cascade")
            .sql_state(),
        "0A000"
    );
    assert_eq!(
        fixture.rows("SELECT * FROM GRAPH_TABLE(alt.g MATCH(v IS node) COLUMNS(v.id)) AS t"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT x FROM public.base"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT x FROM keep.outer_view"),
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        fixture.rows("SELECT x FROM keep.base"),
        vec![vec![Value::Int64(99)]]
    );
}

#[test]
fn should_keep_qualified_graph_and_view_dependencies_precise_during_schema_cascade() {
    // Arrange
    let fixture = started_shadow_fixture("graph-view-qualified-shadow-control", SHADOW_SETUP);
    fixture.execute("CREATE VIEW keep.vg AS SELECT x FROM GRAPH_TABLE(public.g MATCH(v IS node) COLUMNS(v.id AS x)) AS t").expect("qualified direct view");
    fixture
        .execute("CREATE VIEW keep.outer_view AS SELECT x FROM keep.vg")
        .expect("qualified transitive view");
    fixture
        .execute("CREATE VIEW alt.vg AS SELECT 99 AS x")
        .expect("unrelated shadow view");

    // Act
    fixture
        .execute("DROP SCHEMA alt CASCADE")
        .expect("qualified references do not depend on alt graph");
    let remaining = fixture.rows("SELECT x FROM keep.outer_view");
    let blocked = fixture.execute("DROP PROPERTY GRAPH public.g");

    // Assert
    assert_eq!(remaining, vec![vec![Value::Int64(2)]]);
    assert_eq!(
        blocked
            .expect_err("qualified dependency still protects public graph")
            .sql_state(),
        "2BP01"
    );
}
