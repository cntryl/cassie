//! PostgreSQL 18 money arithmetic, aggregate identity and overflow tracked by #908.

use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

struct MoneyDirectory(String);

impl Drop for MoneyDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_preserve_money_arithmetic_precision() {
    // Arrange
    let fixture = sql_fixture("issue-908-money-exact-arithmetic", &[]);
    let sql = "SELECT (12.34::numeric::money + 0.01::numeric::money) = 12.35::numeric::money";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("money arithmetic and aggregate identity")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_parse_money_text_compare_values_propagate_nulls() {
    // Arrange
    let fixture = sql_fixture("issue-908-money-c-locale-comparison-null", &[]);
    let sql = "SELECT '12.34'::money::TEXT, 12.35::money > 12.34::money, (NULL::money + 1::money) IS NULL, (NULL::money = 1::money) IS NULL";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("money text input, output, comparison and NULLs")
            .rows(),
        &[vec![
            Value::String("$12.34".to_owned()),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_sum_multiple_money_values_exactly_skipping_nulls() {
    // Arrange
    let fixture = sql_fixture("issue-908-money-exact-sum", &[]);
    fixture
        .execute("CREATE TABLE issue_908_money_rows (amount MONEY)")
        .expect("create a table with money values");
    fixture
        .execute(
            "INSERT INTO issue_908_money_rows VALUES ('90071992547409.92'::MONEY), (NULL), ('0.01'::MONEY)",
        )
        .expect("insert multiple money values and NULL");

    // Act
    let result = fixture.execute(
        "SELECT SUM(amount)::NUMERIC = 90071992547409.93::NUMERIC, pg_typeof(SUM(amount))::TEXT, MIN(amount)::NUMERIC = 0.01::NUMERIC, MAX(amount)::NUMERIC = 90071992547409.92::NUMERIC FROM issue_908_money_rows",
    );

    // Assert
    assert_eq!(
        result
            .expect("exact money sum across multiple values")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::String("money".to_owned()),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_keep_money_text_values_across_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("issue-908-money-restart");
    let _directory = MoneyDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create money fixture");
        cassie.startup().expect("start money fixture");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_908_restart_money (amount MONEY)",
                vec![],
            )
            .expect("create a money column");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO issue_908_restart_money VALUES ('12.34'::MONEY)",
                vec![],
            )
            .expect("store a money value");
    }
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen money fixture");
    restarted.startup().expect("restart money fixture");
    let session = restarted.create_session("tester", None);

    // Act
    let result = restarted.execute_sql(
        &session,
        "SELECT amount::TEXT FROM issue_908_restart_money",
        vec![],
    );

    // Assert
    assert_eq!(
        result.expect("money value survives restart").rows(),
        &[vec![Value::String("$12.34".to_owned())]]
    );
    drop(session);
    restarted.shutdown();
    drop(restarted);
}
