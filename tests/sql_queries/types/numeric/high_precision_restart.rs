//! PostgreSQL 18 unconstrained NUMERIC persistence boundaries for #812.

use cassie::app::Cassie;
use cassie::types::Value;

struct NumericDirectory(String);

impl Drop for NumericDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_keep_unconstrained_numeric_above_thirty_eight_digits_across_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("issue-812-wide-unconstrained-numeric-restart");
    let _directory = NumericDirectory(path.clone());
    let exact_value = "12345678901234567890123456789012345678901234567890.12345678901234567890";
    let adjacent_value = "12345678901234567890123456789012345678901234567890.12345678901234567889";

    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create numeric fixture");
        cassie.startup().expect("start numeric fixture");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_812_wide_restart_values (amount NUMERIC NOT NULL)",
                vec![],
            )
            .expect("create unconstrained numeric column");
        cassie
            .execute_sql(
                &session,
                &format!("INSERT INTO issue_812_wide_restart_values VALUES ('{exact_value}')"),
                vec![],
            )
            .expect("store exact unconstrained numeric value");
        drop(session);
        cassie.shutdown();
    }

    let restarted = Cassie::new_with_data_dir(&path).expect("reopen numeric fixture");
    restarted.startup().expect("restart numeric fixture");
    let session = restarted.create_session("tester", None);

    // Act
    let result = restarted.execute_sql(
        &session,
        &format!(
            "SELECT amount = CAST('{exact_value}' AS NUMERIC), \
             amount <> CAST('{adjacent_value}' AS NUMERIC) \
             FROM issue_812_wide_restart_values"
        ),
        vec![],
    );

    // Assert
    let result = result.expect("wide numeric value survives restart exactly");
    assert_eq!(result.rows(), &[vec![Value::Bool(true), Value::Bool(true)]]);
    drop(session);
    restarted.shutdown();
}
