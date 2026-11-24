//! Row rewrite, rollback and restart evidence for the qualified `ALTER COLUMN TYPE ... USING` pairs.

use std::fmt::Write as _;

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::types::Value;

use crate::support_catalog_directory::CatalogDirectory;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

#[path = "using_conversion_pairs.rs"]
mod pairs;

use pairs::{Pair, PAIRS};

fn using_expression(pair: &Pair, column: &str) -> String {
    pair.using.replace("{c}", column)
}

fn seeded(pair: &Pair, table: &str) -> SqlFixture {
    sql_fixture(
        "using-pairs",
        &[
            &format!("CREATE TABLE {table} (id INT PRIMARY KEY, c {})", pair.from),
            &format!(
                "INSERT INTO {table} VALUES (1, {}), (2, NULL)",
                pair.literal
            ),
        ],
    )
}

fn alter_sql(pair: &Pair, table: &str) -> String {
    format!(
        "ALTER TABLE {table} ALTER COLUMN c TYPE {} USING {}",
        pair.to,
        using_expression(pair, "c")
    )
}

fn observed(fixture: &SqlFixture, table: &str) -> Vec<Vec<Value>> {
    fixture.rows(&format!(
        "SELECT c::text, pg_typeof(c)::text FROM {table} ORDER BY id"
    ))
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

fn converted(pair: &Pair) -> Vec<Vec<Value>> {
    vec![
        vec![text(pair.value), text(pair.type_name)],
        vec![Value::Null, text(pair.type_name)],
    ]
}

#[test]
fn should_rewrite_rows_for_every_qualified_using_pair() {
    // Arrange
    for pair in PAIRS {
        let fixture = seeded(pair, "using_rows");

        // Act
        let altered = fixture.execute(&alter_sql(pair, "using_rows"));
        let rows = observed(&fixture, "using_rows");

        // Assert
        assert!(
            altered.is_ok(),
            "{} -> {} must be accepted",
            pair.from,
            pair.to
        );
        assert_eq!(rows, converted(pair), "{} -> {}", pair.from, pair.to);
    }
}

#[test]
fn should_restore_rows_given_a_rolled_back_qualified_using_pair() {
    // Arrange
    for pair in PAIRS {
        let fixture = seeded(pair, "using_rollback");
        let before = observed(&fixture, "using_rollback");
        fixture.execute("BEGIN").expect("begin");
        fixture
            .execute(&alter_sql(pair, "using_rollback"))
            .unwrap_or_else(|_| panic!("{} -> {} in a transaction", pair.from, pair.to));
        let staged = observed(&fixture, "using_rollback");

        // Act
        fixture.execute("ROLLBACK").expect("rollback");
        let after = observed(&fixture, "using_rollback");

        // Assert
        assert_eq!(
            staged,
            converted(pair),
            "{} -> {} staged",
            pair.from,
            pair.to
        );
        assert_eq!(after, before, "{} -> {} rolled back", pair.from, pair.to);
    }
}

#[test]
fn should_publish_a_committed_using_pair_to_another_session() {
    // Arrange
    let pair = &PAIRS[3];
    let fixture = seeded(pair, "using_commit");
    let observer = fixture.cassie.create_session("tester", None);
    fixture.execute("BEGIN").expect("begin");
    fixture
        .execute(&alter_sql(pair, "using_commit"))
        .expect("staged rewrite");
    let isolated = fixture
        .cassie
        .execute_sql(
            &observer,
            "SELECT pg_typeof(c)::text FROM using_commit WHERE id = 1",
            vec![],
        )
        .expect("observer before commit");

    // Act
    fixture.execute("COMMIT").expect("commit");
    let published = fixture
        .cassie
        .execute_sql(
            &observer,
            "SELECT c::text, pg_typeof(c)::text FROM using_commit WHERE id = 1",
            vec![],
        )
        .expect("observer after commit");

    // Assert
    assert_eq!(isolated.rows(), &[vec![text("integer")]]);
    assert_eq!(
        published.rows(),
        &[vec![text(pair.value), text(pair.type_name)]]
    );
}

fn representative(from: &str, to: &str) -> &'static Pair {
    PAIRS
        .iter()
        .find(|pair| pair.from == from && pair.to == to)
        .unwrap_or_else(|| panic!("{from} -> {to} is not a qualified pair"))
}

fn restart_pairs() -> Vec<&'static Pair> {
    vec![
        representative("INT", "NUMERIC(20,2)"),
        representative("INT", "TEXT"),
        representative("TEXT", "JSONB"),
        representative("TEXT", "TIMESTAMP"),
        representative("TEXT", "UUID"),
        representative("DOUBLE PRECISION", "NUMERIC(20,2)"),
        representative("BOOLEAN", "INT"),
        representative("JSONB", "TEXT"),
        representative("INT", "INT[]"),
    ]
}

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
}

fn open(directory: &CatalogDirectory) -> (Cassie, CassieSession) {
    let cassie = Cassie::new_with_data_dir_and_config(
        &directory.0,
        cassie::config::CassieRuntimeConfig::default(),
    )
    .expect("open local-storage Cassie");
    cassie.startup().expect("start local-storage Cassie");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

fn restart_columns(pairs: &[&Pair]) -> (String, String, String) {
    let mut definitions = String::new();
    let mut literals = String::new();
    let mut actions = Vec::new();
    for (index, pair) in pairs.iter().enumerate() {
        let _ = write!(definitions, ", c{index} {}", pair.from);
        let _ = write!(literals, ", {}", pair.literal);
        actions.push(format!(
            "ALTER COLUMN c{index} TYPE {} USING {}",
            pair.to,
            using_expression(pair, &format!("c{index}"))
        ));
    }
    (definitions, literals, actions.join(", "))
}

fn restart_projection(count: usize) -> String {
    let columns = (0..count)
        .map(|index| format!("c{index}::text, pg_typeof(c{index})::text"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("SELECT {columns} FROM {{table}} WHERE id = 1")
}

#[test]
fn should_recover_using_rewrite_outcomes_after_local_restart() {
    // Arrange
    let pairs = restart_pairs();
    let (definitions, literals, actions) = restart_columns(&pairs);
    let directory = CatalogDirectory::new("using-pairs-restart");
    let projection = restart_projection(pairs.len());
    let original = {
        let (cassie, session) = open(&directory);
        for table in ["using_committed", "using_discarded"] {
            execute(
                &cassie,
                &session,
                &format!("CREATE TABLE {table} (id INT PRIMARY KEY{definitions})"),
            );
            execute(
                &cassie,
                &session,
                &format!("INSERT INTO {table} VALUES (1{literals})"),
            );
        }
        let original = cassie
            .execute_sql(
                &session,
                &projection.replace("{table}", "using_discarded"),
                vec![],
            )
            .expect("read the original row")
            .rows()
            .to_vec();
        execute(&cassie, &session, "BEGIN");
        execute(
            &cassie,
            &session,
            &format!("ALTER TABLE using_committed {actions}"),
        );
        execute(&cassie, &session, "COMMIT");
        execute(&cassie, &session, "BEGIN");
        execute(
            &cassie,
            &session,
            &format!("ALTER TABLE using_discarded {actions}"),
        );
        execute(&cassie, &session, "ROLLBACK");
        original
    };

    // Act
    let (cassie, session) = open(&directory);
    let read = |table: &str| {
        cassie
            .execute_sql(&session, &projection.replace("{table}", table), vec![])
            .unwrap_or_else(|error| panic!("read {table} after restart: {error}"))
            .rows()
            .to_vec()
    };
    let committed = read("using_committed");
    let discarded = read("using_discarded");

    // Assert
    let expected_committed = vec![pairs
        .iter()
        .flat_map(|pair| [text(pair.value), text(pair.type_name)])
        .collect::<Vec<_>>()];
    assert_eq!(committed, expected_committed);
    assert_eq!(discarded, original);
}

#[test]
fn should_reject_unqualified_using_results_without_partial_rewrite() {
    // Arrange
    let cases: &[(&str, &str, &str, &str)] = &[
        // The USING result is not cast to the target type.
        ("INT", "22", "TEXT", "c"),
        ("TEXT", "'22'", "INT", "c"),
        // A value the cast itself rejects.
        ("TEXT", "'abc'", "INT", "c::int"),
        ("BIGINT", "5000000000", "INT", "c::int"),
    ];
    for (from, literal, to, using) in cases {
        let fixture = sql_fixture(
            "using-reject",
            &[
                &format!("CREATE TABLE using_rejected (id INT PRIMARY KEY, c {from})"),
                &format!("INSERT INTO using_rejected VALUES (1, {literal}), (2, {literal})"),
            ],
        );
        let before = observed(&fixture, "using_rejected");

        // Act
        let rejected = fixture.execute(&format!(
            "ALTER TABLE using_rejected ALTER COLUMN c TYPE {to} USING {using}"
        ));
        let after = observed(&fixture, "using_rejected");
        let followup = fixture.execute("INSERT INTO using_rejected VALUES (3, NULL)");

        // Assert
        assert!(
            matches!(rejected, Err(ref error) if !matches!(error, CassieError::Storage(_))),
            "{from} -> {to} USING {using} must be rejected before any rewrite"
        );
        assert_eq!(
            after, before,
            "{from} -> {to} USING {using} left rows changed"
        );
        assert!(followup.is_ok(), "the table stays writable");
    }
}

#[test]
fn should_require_an_explicit_using_expression_for_a_type_change() {
    // Arrange
    let fixture = sql_fixture(
        "using-required",
        &[
            "CREATE TABLE using_required (id INT PRIMARY KEY, c INT)",
            "INSERT INTO using_required VALUES (1, 22)",
        ],
    );

    // Act
    let rejected = fixture.execute("ALTER TABLE using_required ALTER COLUMN c TYPE BIGINT");
    let rows = observed(&fixture, "using_required");

    // Assert
    assert!(matches!(rejected, Err(CassieError::Unsupported(_))));
    assert_eq!(rows, vec![vec![text("22"), text("integer")]]);
}
