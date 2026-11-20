//! Keyed UPDATE and DELETE fetch only the rows their WHERE clause names.

use cassie::types::Value;
use cassie_test_support::support_sql_fixture::{sql_fixture_with_config, SqlFixture};

const ROWS: i64 = 300;
const LOWERED_BUDGET_BYTES: usize = 48 * 1024;
const BODY: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";

fn fixture(label: &str, budget: Option<usize>) -> SqlFixture {
    let mut config = cassie::config::CassieRuntimeConfig::default();
    if let Some(budget) = budget {
        config.limits.query_memory_budget_bytes = budget;
    }
    sql_fixture_with_config(label, &[], config)
}

fn seed_events(fixture: &SqlFixture, table: &str, indexed: bool) {
    fixture
        .execute(&format!(
            "CREATE TABLE {table} (id INT NOT NULL, owner INT NOT NULL, status TEXT NOT NULL, body TEXT)"
        ))
        .unwrap();
    if indexed {
        for column in ["id", "owner", "status"] {
            fixture
                .execute(&format!(
                    "CREATE INDEX {table}_{column}_idx ON {table} ({column})"
                ))
                .unwrap();
        }
    }
    for chunk in (1..=ROWS).collect::<Vec<_>>().chunks(10) {
        let values = chunk
            .iter()
            .map(|id| {
                let status = if id % 2 == 0 { "open" } else { "closed" };
                format!("({id}, {}, '{status}', '{BODY}')", id % 3)
            })
            .collect::<Vec<_>>()
            .join(", ");
        fixture
            .execute(&format!(
                "INSERT INTO {table} (id, owner, status, body) VALUES {values}"
            ))
            .unwrap();
    }
}

fn counters(fixture: &SqlFixture) -> (u64, u64) {
    let metrics = fixture.cassie.metrics();
    let read = |name: &str| metrics["read_paths"][name].as_u64().unwrap_or_default();
    (
        read("dml_keyed_row_lookups_total"),
        read("dml_full_scan_total"),
    )
}

fn run(fixture: &SqlFixture, sql: &str, params: Vec<Value>) -> (String, Vec<Vec<Value>>) {
    let result = fixture
        .cassie
        .execute_sql(&fixture.session, sql, params)
        .unwrap_or_else(|error| panic!("statement failed: {sql}: {error}"));
    (result.command().to_string(), result.rows().to_vec())
}

fn contents(fixture: &SqlFixture, table: &str) -> Vec<Vec<Value>> {
    fixture.rows(&format!(
        "SELECT id, owner, status, body FROM {table} ORDER BY id"
    ))
}

#[test]
fn should_update_one_indexed_row_within_a_lowered_memory_budget() {
    // Arrange
    let fixture = fixture("keyed_update_budget", Some(LOWERED_BUDGET_BYTES));
    seed_events(&fixture, "keyed_events", true);
    seed_events(&fixture, "scan_events", false);
    let before = counters(&fixture);

    // Act
    let keyed = run(
        &fixture,
        "UPDATE keyed_events SET status = 'done' WHERE id = $1",
        vec![Value::Int64(7)],
    );
    let after = counters(&fixture);
    let scanned = fixture
        .execute("UPDATE scan_events SET status = 'done' WHERE id = 7")
        .is_err_and(|error| error.to_string().contains("query memory budget exceeded"));

    // Assert
    assert_eq!(keyed.0, "UPDATE 1");
    assert_eq!(after, (before.0 + 1, before.1));
    assert!(scanned, "the control table must exceed the lowered budget");
    assert_eq!(
        fixture.rows("SELECT status FROM keyed_events WHERE id = 7"),
        vec![vec![Value::String("done".to_string())]]
    );
}

#[test]
fn should_delete_one_indexed_row_within_a_lowered_memory_budget() {
    // Arrange
    let fixture = fixture("keyed_delete_budget", Some(LOWERED_BUDGET_BYTES));
    seed_events(&fixture, "keyed_events", true);
    let before = counters(&fixture);

    // Act
    let keyed = run(
        &fixture,
        "DELETE FROM keyed_events WHERE id = $1",
        vec![Value::Int64(9)],
    );
    let after = counters(&fixture);

    // Assert
    assert_eq!(keyed.0, "DELETE 1");
    assert_eq!(after, (before.0 + 1, before.1));
    assert_eq!(
        fixture
            .rows("SELECT id FROM keyed_events WHERE id = 9")
            .len(),
        0
    );
}

#[test]
fn should_scan_when_no_equality_conjunct_has_an_index() {
    // Arrange
    let fixture = fixture("keyed_no_index", None);
    seed_events(&fixture, "scan_events", false);
    let before = counters(&fixture);

    // Act
    let updated = run(
        &fixture,
        "UPDATE scan_events SET status = 'done' WHERE id = 7 OR id = 8",
        vec![],
    );
    let after = counters(&fixture);

    // Assert
    assert_eq!(updated.0, "UPDATE 2");
    assert_eq!(after, (before.0, before.1 + 1));
}

#[test]
fn should_point_read_the_row_identity() {
    // Arrange
    let fixture = fixture("keyed_identity", Some(LOWERED_BUDGET_BYTES));
    seed_events(&fixture, "scan_events", true);
    let identity = fixture.rows("SELECT _id FROM scan_events WHERE id = 11")[0][0].clone();
    let before = counters(&fixture);

    // Act
    let updated = run(
        &fixture,
        "UPDATE scan_events SET status = 'done' WHERE _id = $1 RETURNING id, status",
        vec![identity.clone()],
    );
    let deleted = run(
        &fixture,
        "DELETE FROM scan_events WHERE _id = $1 RETURNING id",
        vec![identity],
    );
    let after = counters(&fixture);

    // Assert
    assert_eq!(
        updated.1,
        vec![vec![Value::Int64(11), Value::String("done".to_string())]]
    );
    assert_eq!(deleted.1, vec![vec![Value::Int64(11)]]);
    assert_eq!(after, (before.0 + 2, before.1));
}

#[test]
fn should_intersect_two_indexes_with_a_residual_filter() {
    // Arrange
    let fixture = fixture("keyed_intersection", None);
    seed_events(&fixture, "keyed_events", true);
    let before = counters(&fixture);

    // Act
    let updated = run(
        &fixture,
        "UPDATE keyed_events SET body = 'seen' WHERE owner = 1 AND status = 'open' AND id > 290 RETURNING id",
        vec![],
    );
    let after = counters(&fixture);

    // Assert
    let ids = updated
        .1
        .iter()
        .map(|row| row[0].clone())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec![Value::Int64(292), Value::Int64(298)]);
    assert_eq!(after, (before.0 + 1, before.1));
}

fn equivalence_script(table: &str) -> Vec<(String, Vec<Value>)> {
    vec![
        (
            format!("UPDATE {table} SET status = 'a' WHERE id = $1 RETURNING id, owner, status"),
            vec![Value::Int64(5)],
        ),
        (
            format!("UPDATE {table} SET owner = owner + 10 WHERE owner = 1 AND status = 'open' AND id < 40 RETURNING id, owner"),
            vec![],
        ),
        (
            format!("DELETE FROM {table} WHERE id = 6 AND status = 'closed' RETURNING id"),
            vec![],
        ),
        (
            format!("DELETE FROM {table} WHERE id = 7 AND status = 'closed' RETURNING id"),
            vec![],
        ),
        (
            format!("DELETE FROM {table} WHERE status = 'open' AND id = 10 RETURNING id, status"),
            vec![],
        ),
        (format!("UPDATE {table} SET body = 'x' WHERE id = 100000"), vec![]),
        (format!("UPDATE {table} SET body = 'x' WHERE id = NULL"), vec![]),
        (format!("UPDATE {table} SET body = 'y' WHERE id = '12'"), vec![]),
        (format!("DELETE FROM {table} WHERE id = '13'"), vec![]),
    ]
}

#[test]
fn should_match_the_scan_path_for_keyed_statements() {
    // Arrange
    let fixture = fixture("keyed_equivalence", None);
    seed_events(&fixture, "keyed_events", true);
    seed_events(&fixture, "scan_events", false);

    // Act
    let keyed = equivalence_script("keyed_events")
        .into_iter()
        .map(|(sql, params)| run(&fixture, &sql, params))
        .collect::<Vec<_>>();
    let scanned = equivalence_script("scan_events")
        .into_iter()
        .map(|(sql, params)| run(&fixture, &sql, params))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(keyed, scanned);
    assert_eq!(
        contents(&fixture, "keyed_events"),
        contents(&fixture, "scan_events")
    );
    assert_eq!(keyed[0].0, "UPDATE 1");
    assert_eq!(keyed[2].0, "DELETE 0");
    assert_eq!(keyed[3].0, "DELETE 1");
    assert_eq!(keyed[5].0, "UPDATE 0");
    assert_eq!(keyed[6].0, "UPDATE 0");
}

fn transaction_script(table: &str) -> Vec<String> {
    vec![
        "BEGIN".to_string(),
        format!("INSERT INTO {table} (id, owner, status, body) VALUES (1001, 1, 'open', 'new')"),
        format!("UPDATE {table} SET status = 'mid' WHERE id = 1001 RETURNING id, status"),
        format!("UPDATE {table} SET status = 'mid' WHERE id = 3 RETURNING id, status"),
        format!("DELETE FROM {table} WHERE id = 3 RETURNING id, status"),
        format!("DELETE FROM {table} WHERE id = 1001 RETURNING id, status"),
        format!("UPDATE {table} SET status = 'late' WHERE id = 4 RETURNING id, status"),
        "COMMIT".to_string(),
        "BEGIN".to_string(),
        format!(
            "UPDATE {table} SET status = 'gone' WHERE owner = 2 AND status = 'closed' RETURNING id"
        ),
        format!("DELETE FROM {table} WHERE id = 20 RETURNING id"),
        "ROLLBACK".to_string(),
    ]
}

#[test]
fn should_match_the_scan_path_inside_open_transactions() {
    // Arrange
    let fixture = fixture("keyed_transactions", None);
    seed_events(&fixture, "keyed_events", true);
    seed_events(&fixture, "scan_events", false);

    // Act
    let keyed = transaction_script("keyed_events")
        .iter()
        .map(|sql| run(&fixture, sql, vec![]))
        .collect::<Vec<_>>();
    let scanned = transaction_script("scan_events")
        .iter()
        .map(|sql| run(&fixture, sql, vec![]))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(keyed, scanned);
    assert_eq!(
        contents(&fixture, "keyed_events"),
        contents(&fixture, "scan_events")
    );
    assert_eq!(
        fixture
            .rows("SELECT id FROM keyed_events WHERE id = 1001")
            .len(),
        0
    );
    assert_eq!(
        fixture.rows("SELECT status FROM keyed_events WHERE id = 4"),
        vec![vec![Value::String("late".to_string())]]
    );
}

#[test]
fn should_scan_for_a_table_changed_by_the_open_transaction() {
    // Arrange
    let fixture = fixture("keyed_overlay_scan", None);
    seed_events(&fixture, "keyed_events", true);
    run(&fixture, "BEGIN", vec![]);
    run(
        &fixture,
        "INSERT INTO keyed_events (id, owner, status, body) VALUES (2001, 0, 'open', 'new')",
        vec![],
    );
    let before = counters(&fixture);

    // Act
    let updated = run(
        &fixture,
        "UPDATE keyed_events SET status = 'x' WHERE id = 2001",
        vec![],
    );
    let after = counters(&fixture);
    run(&fixture, "ROLLBACK", vec![]);

    // Assert
    assert_eq!(updated.0, "UPDATE 1");
    assert_eq!(after, (before.0, before.1 + 1));
}

#[test]
fn should_match_the_scan_path_for_float_signed_zero_keys() {
    // Arrange
    let fixture = fixture("keyed_signed_zero", None);
    for table in ["keyed_floats", "scan_floats"] {
        fixture
            .execute(&format!("CREATE TABLE {table} (k FLOAT NOT NULL, v TEXT)"))
            .unwrap();
        fixture
            .execute(&format!(
                "INSERT INTO {table} (k, v) VALUES (-0.0, 'neg'), (1.5, 'one'), (2.5, 'two')"
            ))
            .unwrap();
    }
    fixture
        .execute("CREATE INDEX keyed_floats_k_idx ON keyed_floats (k)")
        .unwrap();

    // Act
    let outcome = |table: &str| {
        [
            format!("UPDATE {table} SET v = 'zero' WHERE k = 0.0 RETURNING v"),
            format!("UPDATE {table} SET v = 'int' WHERE k = $1 RETURNING v"),
            format!("DELETE FROM {table} WHERE k = 1.5 RETURNING v"),
        ]
        .iter()
        .enumerate()
        .map(|(position, sql)| {
            let params = if position == 1 {
                vec![Value::Float64(-0.0)]
            } else {
                vec![]
            };
            run(&fixture, sql, params)
        })
        .collect::<Vec<_>>()
    };
    let keyed = outcome("keyed_floats");
    let scanned = outcome("scan_floats");

    // Assert
    assert_eq!(keyed, scanned);
    assert_eq!(keyed[0].0, "UPDATE 1");
    assert_eq!(keyed[2].0, "DELETE 1");
}
