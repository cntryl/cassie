use super::super::support_sql_fixture::sql_fixture;
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

const RELATION_NAME: &str = "issue833_read_committed_relation_visibility";

fn visible_relation(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            &format!(
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = 'public' \
                   AND table_name = '{RELATION_NAME}'"
            ),
            vec![],
        )
        .expect("read the relation from information_schema.tables")
        .rows()
        .to_vec()
}

#[test]
fn should_observe_committed_relation_after_each_read_committed_statement() {
    // Arrange
    let fixture = sql_fixture("issue-833-read-committed-catalog-create", &[]);
    let reader = fixture.session.clone();
    let writer = fixture.cassie.create_session("writer", None);
    fixture
        .cassie
        .execute_sql(
            &reader,
            "BEGIN TRANSACTION ISOLATION LEVEL READ COMMITTED",
            vec![],
        )
        .expect("begin a READ COMMITTED transaction");
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
    let observer = fixture.cassie.create_session("observer", None);
    let observer_rows = visible_relation(&fixture.cassie, &observer);
    let after_create = visible_relation(&fixture.cassie, &reader);
    fixture
        .cassie
        .execute_sql(&reader, "COMMIT", vec![])
        .expect("finish the READ COMMITTED transaction");

    // Assert
    let no_visible_relation: &[Vec<Value>] = &[];
    assert_eq!(before_create, no_visible_relation);
    assert_eq!(
        observer_rows,
        vec![vec![Value::String(RELATION_NAME.to_owned())]],
        "a new session sees the committed relation"
    );
    assert_eq!(
        after_create,
        vec![vec![Value::String(RELATION_NAME.to_owned())]],
        "the next READ COMMITTED statement observes the committed relation"
    );
}
