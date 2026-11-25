//! Transactional ALTER catalog and row rollback tracked by #833.

use cassie::types::Value;

use super::super::support_sql_fixture::sql_fixture;

#[test]
fn should_restore_original_table_after_transactional_alter_rollback() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-alter-catalog-data-rollback",
        &[
            "CREATE TABLE public.issue833_alter_rollback_rows \
               (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO public.issue833_alter_rollback_rows VALUES (1, 'retained')",
        ],
    );
    let observer = fixture.cassie.create_session("observer", None);
    fixture.execute("BEGIN").expect("begin the DDL transaction");
    fixture
        .execute("ALTER TABLE public.issue833_alter_rollback_rows ADD COLUMN note TEXT")
        .expect("add a column inside the transaction");
    fixture
        .execute(
            "UPDATE public.issue833_alter_rollback_rows \
             SET label = 'uncommitted', note = 'uncommitted' WHERE id = 1",
        )
        .expect("update the row under the new descriptor");

    // Act
    let writer_columns = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'issue833_alter_rollback_rows' \
             ORDER BY ordinal_position",
            vec![],
        )
        .expect("read the writer's transactional descriptor")
        .rows()
        .to_vec();
    let writer_rows = fixture
        .execute("SELECT id, label, note FROM public.issue833_alter_rollback_rows ORDER BY id")
        .expect("read the writer's transactional row")
        .rows()
        .to_vec();
    fixture
        .execute("ROLLBACK")
        .expect("roll back both catalog and row changes");
    let observer_columns = fixture
        .cassie
        .execute_sql(
            &observer,
            "SELECT column_name FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'issue833_alter_rollback_rows' \
             ORDER BY ordinal_position",
            vec![],
        )
        .expect("read the descriptor after rollback from a second session")
        .rows()
        .to_vec();
    let observer_rows = fixture
        .cassie
        .execute_sql(
            &observer,
            "SELECT id, label FROM public.issue833_alter_rollback_rows ORDER BY id",
            vec![],
        )
        .expect("read the restored row from a second session")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(
        writer_columns,
        vec![
            vec![Value::String("id".to_owned())],
            vec![Value::String("label".to_owned())],
            vec![Value::String("note".to_owned())],
        ]
    );
    assert_eq!(
        writer_rows,
        vec![vec![
            Value::Int64(1),
            Value::String("uncommitted".to_owned()),
            Value::String("uncommitted".to_owned()),
        ]]
    );
    assert_eq!(
        observer_columns,
        vec![
            vec![Value::String("id".to_owned())],
            vec![Value::String("label".to_owned())],
        ]
    );
    assert_eq!(
        observer_rows,
        vec![vec![Value::Int64(1), Value::String("retained".to_owned())]]
    );
}
