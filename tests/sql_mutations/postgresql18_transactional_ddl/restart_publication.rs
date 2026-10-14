use super::super::support_sql as support;
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

struct DataDirectory(String);

impl Drop for DataDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("statement failed: {sql}: {error}"));
}

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("query failed: {sql}: {error}"))
        .rows()
        .to_vec()
}

#[test]
fn should_preserve_transactional_catalog_publication_across_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("issue-833-transactional-catalog-restart");
    let _directory = DataDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create first Cassie instance");
        cassie.startup().expect("start first Cassie instance");
        let session = cassie.create_session("tester", None);
        execute(
            &cassie,
            &session,
            "CREATE TABLE public.issue833_restart_altered_rows \
               (id INT PRIMARY KEY, label TEXT)",
        );
        execute(
            &cassie,
            &session,
            "INSERT INTO public.issue833_restart_altered_rows VALUES (1, 'before')",
        );

        // Act
        execute(&cassie, &session, "BEGIN");
        execute(
            &cassie,
            &session,
            "ALTER TABLE public.issue833_restart_altered_rows ADD COLUMN note TEXT",
        );
        execute(
            &cassie,
            &session,
            "UPDATE public.issue833_restart_altered_rows SET note = 'committed' WHERE id = 1",
        );
        execute(&cassie, &session, "COMMIT");

        execute(&cassie, &session, "BEGIN");
        execute(
            &cassie,
            &session,
            "CREATE TABLE public.issue833_restart_committed_rows \
               (id INT PRIMARY KEY, label TEXT NOT NULL)",
        );
        execute(
            &cassie,
            &session,
            "INSERT INTO public.issue833_restart_committed_rows VALUES (2, 'committed create')",
        );
        execute(&cassie, &session, "COMMIT");

        execute(&cassie, &session, "BEGIN");
        execute(
            &cassie,
            &session,
            "CREATE TABLE public.issue833_restart_rolled_back_rows (id INT)",
        );
        execute(&cassie, &session, "ROLLBACK");

        execute(&cassie, &session, "BEGIN");
        execute(
            &cassie,
            &session,
            "DROP TABLE public.issue833_restart_altered_rows",
        );
        execute(&cassie, &session, "ROLLBACK");
    }

    let reopened = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    reopened
        .startup()
        .expect("recover committed catalog publication");
    let session = reopened.create_session("tester", None);
    let altered_rows = rows(
        &reopened,
        &session,
        "SELECT id, label, note FROM public.issue833_restart_altered_rows ORDER BY id",
    );
    let altered_columns = rows(
        &reopened,
        &session,
        "SELECT column_name, data_type, is_nullable \
         FROM information_schema.columns \
         WHERE table_schema = 'public' \
           AND table_name = 'issue833_restart_altered_rows' \
         ORDER BY ordinal_position",
    );
    let created_rows = rows(
        &reopened,
        &session,
        "SELECT id, label FROM public.issue833_restart_committed_rows ORDER BY id",
    );
    let rolled_back_table_count = rows(
        &reopened,
        &session,
        "SELECT count(*) FROM information_schema.tables \
         WHERE table_schema = 'public' \
           AND table_name = 'issue833_restart_rolled_back_rows'",
    );

    // Assert
    assert_eq!(
        altered_rows,
        vec![vec![
            Value::Int64(1),
            Value::String("before".to_owned()),
            Value::String("committed".to_owned()),
        ]]
    );
    assert_eq!(
        altered_columns,
        vec![
            vec![
                Value::String("id".to_owned()),
                Value::String("integer".to_owned()),
                Value::String("NO".to_owned()),
            ],
            vec![
                Value::String("label".to_owned()),
                Value::String("text".to_owned()),
                Value::String("YES".to_owned()),
            ],
            vec![
                Value::String("note".to_owned()),
                Value::String("text".to_owned()),
                Value::String("YES".to_owned()),
            ],
        ]
    );
    assert_eq!(
        created_rows,
        vec![vec![
            Value::Int64(2),
            Value::String("committed create".to_owned()),
        ]]
    );
    assert_eq!(rolled_back_table_count, vec![vec![Value::Int64(0)]]);
}
