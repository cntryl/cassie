//! Transactional staging-table replacement for the v0.1.0 rebuild workflow.

use super::super::support_sql_fixture::sql_fixture;
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("query failed: {sql}: {error}"))
        .rows()
        .to_vec()
}

fn catalog(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    rows(
        cassie,
        session,
        "SELECT relname, oid::BIGINT FROM pg_catalog.pg_class \
         WHERE relname IN ('rebuild_live', 'rebuild_old', 'rebuild_stage') \
         ORDER BY relname",
    )
}

fn renamed_catalog(live_oid: &Value, stage_oid: &Value) -> Vec<Vec<Value>> {
    vec![
        vec![Value::String("rebuild_live".to_owned()), stage_oid.clone()],
        vec![Value::String("rebuild_old".to_owned()), live_oid.clone()],
    ]
}

#[test]
fn should_publish_staging_table_rename_swap_only_after_commit() {
    // Arrange
    let fixture = sql_fixture(
        "issue-826-rebuild-swap",
        &[
            "CREATE TABLE rebuild_live (id BIGINT, label TEXT)",
            "CREATE TABLE rebuild_stage (id BIGINT, label TEXT)",
            "INSERT INTO rebuild_live VALUES (1, 'old')",
            "INSERT INTO rebuild_stage VALUES (2, 'new')",
        ],
    );
    let observer = fixture.cassie.create_session("observer", None);
    let original_catalog = catalog(&fixture.cassie, &observer);
    assert_eq!(original_catalog.len(), 2);
    assert_eq!(
        original_catalog[0][0],
        Value::String("rebuild_live".to_owned())
    );
    assert_eq!(
        original_catalog[1][0],
        Value::String("rebuild_stage".to_owned())
    );
    let live_oid = original_catalog[0][1].clone();
    let stage_oid = original_catalog[1][1].clone();
    assert_ne!(live_oid, stage_oid);

    // Act
    fixture.execute("BEGIN").expect("begin rebuild swap");
    fixture
        .execute("ALTER TABLE rebuild_live RENAME TO rebuild_old")
        .expect("stage old table rename");
    fixture
        .execute("ALTER TABLE rebuild_stage RENAME TO rebuild_live")
        .expect("stage replacement table rename");
    let owner_live = fixture.rows("SELECT id, label FROM rebuild_live");
    let owner_old = fixture.rows("SELECT id, label FROM rebuild_old");
    let owner_catalog = catalog(&fixture.cassie, &fixture.session);
    let observer_before = rows(
        &fixture.cassie,
        &observer,
        "SELECT id, label FROM rebuild_live",
    );
    let observer_stage = rows(
        &fixture.cassie,
        &observer,
        "SELECT id, label FROM rebuild_stage",
    );
    let observer_catalog = catalog(&fixture.cassie, &observer);
    fixture.execute("ROLLBACK").expect("rollback rebuild swap");
    let rolled_back_live = fixture.rows("SELECT id, label FROM rebuild_live");
    let rolled_back_stage = fixture.rows("SELECT id, label FROM rebuild_stage");
    let rolled_back_catalog = catalog(&fixture.cassie, &observer);
    fixture.execute("BEGIN").expect("retry rebuild swap");
    fixture
        .execute("ALTER TABLE rebuild_live RENAME TO rebuild_old")
        .expect("retry old table rename");
    fixture
        .execute("ALTER TABLE rebuild_stage RENAME TO rebuild_live")
        .expect("retry replacement table rename");
    fixture.execute("COMMIT").expect("commit rebuild swap");
    let committed_live = rows(
        &fixture.cassie,
        &observer,
        "SELECT id, label FROM rebuild_live",
    );
    let committed_old = rows(
        &fixture.cassie,
        &observer,
        "SELECT id, label FROM rebuild_old",
    );
    let committed_catalog = catalog(&fixture.cassie, &observer);

    // Assert
    let old = vec![vec![Value::Int64(1), Value::String("old".to_owned())]];
    let new = vec![vec![Value::Int64(2), Value::String("new".to_owned())]];
    let swapped_catalog = renamed_catalog(&live_oid, &stage_oid);
    assert_eq!(owner_live, new);
    assert_eq!(owner_old, old);
    assert_eq!(owner_catalog, swapped_catalog);
    assert_eq!(observer_before, old);
    assert_eq!(observer_stage, new);
    assert_eq!(observer_catalog, original_catalog);
    assert_eq!(rolled_back_live, old);
    assert_eq!(rolled_back_stage, new);
    assert_eq!(rolled_back_catalog, original_catalog);
    assert_eq!(committed_live, new);
    assert_eq!(committed_old, old);
    assert_eq!(committed_catalog, swapped_catalog);
}
