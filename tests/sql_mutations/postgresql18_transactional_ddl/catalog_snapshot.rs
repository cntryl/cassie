use super::super::support_sql_fixture::sql_fixture;
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

const RELATION_NAME: &str = "issue833_rr_relation_snapshot";

fn visible_relation(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            "SELECT table_name FROM information_schema.tables \
             WHERE table_schema = 'public' \
               AND table_name = 'issue833_rr_relation_snapshot'",
            vec![],
        )
        .expect("read the relation from information_schema.tables")
        .rows()
        .to_vec()
}

#[test]
fn should_keep_repeatable_read_catalog_snapshot_after_concurrent_relation_create() {
    // Arrange
    let fixture = sql_fixture("issue-833-repeatable-read-catalog-create", &[]);
    let reader = fixture.session.clone();
    let writer = fixture.cassie.create_session("writer", None);
    fixture
        .cassie
        .execute_sql(
            &reader,
            "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            vec![],
        )
        .expect("begin the catalog snapshot");
    let before_create = visible_relation(&fixture.cassie, &reader);

    // Act
    fixture
        .cassie
        .execute_sql(
            &writer,
            &format!("CREATE TABLE public.{RELATION_NAME} (id INTEGER PRIMARY KEY)"),
            vec![],
        )
        .expect("create and publish the relation in another session");
    let inside_snapshot_after_create = visible_relation(&fixture.cassie, &reader);
    fixture
        .cassie
        .execute_sql(&reader, "COMMIT", vec![])
        .expect("finish the repeatable-read transaction");
    let after_reader_commit = visible_relation(&fixture.cassie, &reader);

    // Assert
    let no_visible_relation: &[Vec<Value>] = &[];
    assert_eq!(before_create, no_visible_relation);
    assert_eq!(
        inside_snapshot_after_create, no_visible_relation,
        "a committed CREATE TABLE remains outside the existing repeatable-read catalog snapshot"
    );
    assert_eq!(
        after_reader_commit,
        vec![vec![Value::String(RELATION_NAME.to_owned())]],
        "the new relation becomes visible after the reader starts a fresh transaction"
    );
}
