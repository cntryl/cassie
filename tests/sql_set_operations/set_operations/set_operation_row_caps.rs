use cassie::app::{Cassie, CassieSession};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

fn capped_instance(label: &str) -> (Cassie, CassieSession, String) {
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir(label);
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 5;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let mut setup = vec![
        "CREATE TABLE smallt (n INT)".to_string(),
        "CREATE TABLE biggt (n INT)".to_string(),
        "INSERT INTO smallt (n) VALUES (0)".to_string(),
        "INSERT INTO smallt (n) VALUES (19)".to_string(),
    ];
    setup.extend((0..20).map(|n| format!("INSERT INTO biggt (n) VALUES ({n})")));
    for sql in &setup {
        assert!(
            cassie.execute_sql(&session, sql, vec![]).is_ok(),
            "setup failed: {sql}"
        );
    }
    (cassie, session, path)
}

fn sorted_ints(rows: Vec<Vec<Value>>) -> Vec<i64> {
    let mut values: Vec<i64> = rows
        .into_iter()
        .map(|row| match row.first() {
            Some(Value::Int64(value)) => *value,
            other => panic!("expected an integer cell, got {other:?}"),
        })
        .collect();
    values.sort_unstable();
    values
}

#[test]
fn should_materialize_the_whole_right_branch_under_a_result_row_cap() {
    // Arrange
    let (cassie, session, path) = capped_instance("set_right_cap");
    let run = |sql: &str| {
        let Ok(result) = cassie.execute_sql(&session, sql, vec![]) else {
            panic!("query failed: {sql}");
        };
        sorted_ints(result.rows().to_vec())
    };

    // Act
    let except = run("SELECT n FROM smallt EXCEPT SELECT n FROM biggt");
    let intersect = run("SELECT n FROM smallt INTERSECT SELECT n FROM biggt");
    let filtered = run("SELECT n FROM smallt INTERSECT SELECT n FROM biggt WHERE n >= 0");
    let big_left = run("SELECT n FROM biggt INTERSECT SELECT n FROM smallt");
    let cte_right =
        run("WITH b AS (SELECT n FROM biggt) SELECT n FROM smallt EXCEPT SELECT n FROM b");
    let derived_right = run("SELECT n FROM smallt EXCEPT SELECT n FROM (SELECT n FROM biggt) AS d");
    let recursive_right = run(
        "WITH RECURSIVE r(n) AS (SELECT 0 UNION ALL SELECT n + 1 FROM r WHERE n < 19) \
             SELECT n FROM smallt EXCEPT SELECT n FROM r",
    );

    // Assert
    assert_eq!(except, Vec::<i64>::new());
    assert_eq!(intersect, vec![0, 19]);
    assert_eq!(filtered, vec![0, 19]);
    assert_eq!(big_left, vec![0, 19]);
    assert_eq!(cte_right, Vec::<i64>::new());
    assert_eq!(derived_right, Vec::<i64>::new());
    assert_eq!(recursive_right, Vec::<i64>::new());
    let _ = std::fs::remove_dir_all(path);
}
