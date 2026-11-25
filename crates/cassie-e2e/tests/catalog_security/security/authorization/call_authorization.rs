use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture_with_config;

#[test]
fn should_reject_read_only_call_before_a_procedure_can_write_rows() {
    // Arrange
    let admin_password = uuid::Uuid::new_v4().to_string();
    let reader_password = uuid::Uuid::new_v4().to_string();
    let fixture = sql_fixture_with_config(
        "readonly_call_no_effect",
        &[],
        CassieRuntimeConfig {
            password: admin_password.clone(),
            ..CassieRuntimeConfig::default()
        },
    );
    fixture.cassie.startup().expect("startup");
    let admin = fixture
        .cassie
        .authenticate_role("root", Some(&admin_password), None)
        .expect("actual admin authentication");
    for sql in [
        "CREATE TABLE auth_call_rows (marker TEXT)",
        r#"CREATE PROCEDURE auth_store(marker TEXT) AS "INSERT INTO auth_call_rows (marker) VALUES ($1)""#,
    ] {
        fixture
            .cassie
            .execute_sql(&admin, sql, vec![])
            .expect("admin setup");
    }
    fixture
        .cassie
        .execute_sql(
            &admin,
            &format!("CREATE ROLE call_reader LOGIN PASSWORD '{reader_password}'"),
            vec![],
        )
        .expect("create call reader");
    fixture
        .cassie
        .execute_sql(
            &admin,
            "GRANT CONNECT ON DATABASE postgres TO call_reader",
            vec![],
        )
        .expect("grant call reader access");
    let allowed = fixture
        .cassie
        .execute_sql(&admin, "CALL auth_store('allowed')", vec![])
        .expect("positive control: procedure exists and writes");
    assert_eq!(allowed.command(), "CALL");
    let expected = vec![vec![Value::String("allowed".into())]];
    assert_eq!(
        fixture
            .cassie
            .execute_sql(
                &admin,
                "SELECT marker FROM auth_call_rows ORDER BY marker",
                vec![]
            )
            .expect("baseline")
            .rows(),
        expected
    );
    let reader = fixture
        .cassie
        .authenticate_role("call_reader", Some(&reader_password), None)
        .expect("actual read-only authentication");

    // Act
    let denied = fixture
        .cassie
        .execute_sql(&reader, "CALL auth_store('blocked')", vec![]);

    // Assert
    assert!(matches!(denied, Err(CassieError::InsufficientPrivilege)));
    assert_eq!(
        fixture
            .cassie
            .execute_sql(
                &admin,
                "SELECT marker FROM auth_call_rows ORDER BY marker",
                vec![]
            )
            .expect("post-denial rows")
            .rows(),
        expected,
        "denial occurs before any procedure side effect"
    );
    drop(reader);
    drop(admin);
    fixture.cassie.shutdown();
    drop(fixture);
}
