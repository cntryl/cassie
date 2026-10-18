use cassie::app::{Cassie, CassieSession};

use super::support_sql as support;
use support::{data_dir, use_local_storage};

const ATTEMPTS: usize = 24;

fn start(label: &str) -> (Cassie, CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session, path)
}

fn commits(cassie: &Cassie, session: &CassieSession, statements: &[String]) -> bool {
    statements
        .iter()
        .all(|sql| cassie.execute_sql(session, sql, vec![]).is_ok())
}

#[test]
fn should_commit_delete_then_reinsert_of_a_unique_value_in_any_row_order() {
    // Arrange
    let (cassie, session, path) = start("staged-unique-delete-reinsert");
    let _ = cassie.execute_sql(&session, "CREATE TABLE t (id INT, n INT UNIQUE)", vec![]);

    // Act
    let committed = (0..ATTEMPTS)
        .filter(|attempt| {
            let old = attempt * 2;
            let new = old + 1;
            commits(
                &cassie,
                &session,
                &[
                    format!("INSERT INTO t (id, n) VALUES ({old}, 5)"),
                    "BEGIN".to_string(),
                    format!("DELETE FROM t WHERE id = {old}"),
                    format!("INSERT INTO t (id, n) VALUES ({new}, 5)"),
                    "COMMIT".to_string(),
                    format!("DELETE FROM t WHERE id = {new}"),
                ],
            )
        })
        .count();

    // Assert
    assert_eq!(
        committed, ATTEMPTS,
        "every attempt commits deterministically"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_commit_update_then_reinsert_of_a_unique_value_in_any_row_order() {
    // Arrange
    let (cassie, session, path) = start("staged-unique-update-reinsert");
    let _ = cassie.execute_sql(&session, "CREATE TABLE t (id INT, n INT UNIQUE)", vec![]);

    // Act
    let committed = (0..ATTEMPTS)
        .filter(|attempt| {
            let old = attempt * 2;
            let new = old + 1;
            commits(
                &cassie,
                &session,
                &[
                    format!("INSERT INTO t (id, n) VALUES ({old}, 5)"),
                    "BEGIN".to_string(),
                    format!("UPDATE t SET n = -{old} - 1 WHERE id = {old}"),
                    format!("INSERT INTO t (id, n) VALUES ({new}, 5)"),
                    "COMMIT".to_string(),
                    format!("DELETE FROM t WHERE id = {new}"),
                ],
            )
        })
        .count();

    // Assert
    assert_eq!(
        committed, ATTEMPTS,
        "every attempt commits deterministically"
    );
    let _ = std::fs::remove_dir_all(path);
}
