use cassie::app::Cassie;
use cassie::types::Value;

fn cassie_and_session(_label: &str) -> (Cassie, cassie::CassieSession) {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    let session = cassie.create_session("root", Some("postgres".to_string()));
    (cassie, session)
}

#[test]
fn should_use_one_registry_for_setting_reads() {
    // Arrange
    let (cassie, session) = cassie_and_session("shared_registry");

    // Act
    cassie
        .execute_sql(&session, "SET application_name='DBeaver 26.1.3'", vec![])
        .expect("set application name");
    let shown = cassie
        .execute_sql(&session, "SHOW application_name", vec![])
        .expect("show application name");
    let current = cassie
        .execute_sql(
            &session,
            "SELECT current_setting('application_name')",
            vec![],
        )
        .expect("current_setting");
    let catalog = cassie
        .execute_sql(
            &session,
            "SELECT setting FROM pg_catalog.pg_settings WHERE name = 'application_name'",
            vec![],
        )
        .expect("pg_settings");

    // Assert
    assert_eq!(
        shown.rows()[0][0],
        Value::String("DBeaver 26.1.3".to_string())
    );
    assert_eq!(
        current.rows()[0][0],
        Value::String("DBeaver 26.1.3".to_string())
    );
    assert_eq!(catalog.rows(), current.rows());
}

#[test]
fn should_replay_exact_pgadmin_initialization_settings() {
    // Arrange
    let (cassie, session) = cassie_and_session("pgadmin_init");

    // Act
    cassie
        .execute_sql(&session, "SET DateStyle=ISO", vec![])
        .expect("DateStyle");
    cassie
        .execute_sql(&session, "SET client_min_messages=notice", vec![])
        .expect("messages");
    let configured = cassie.execute_sql(
            &session,
            "SELECT set_config('bytea_output','hex',false) FROM pg_show_all_settings() WHERE name='bytea_output'",
            vec![],
        ).expect("bytea_output initialization");
    cassie
        .execute_sql(&session, "SET client_encoding='UTF8'", vec![])
        .expect("encoding");

    // Assert
    assert_eq!(
        configured.rows(),
        vec![vec![Value::String("hex".to_string())]]
    );
    assert_eq!(session.setting("datestyle").unwrap(), "ISO, MDY");
    assert_eq!(session.setting("client_min_messages").unwrap(), "notice");
    assert_eq!(session.setting("client_encoding").unwrap(), "UTF8");
}

#[test]
fn should_accept_postgres_set_time_zone_syntax() {
    // Arrange
    let (cassie, session) = cassie_and_session("set_time_zone");

    // Act
    let result = cassie.execute_sql(&session, "SET TIME ZONE 'UTC'", vec![]);

    // Assert
    result.expect("set PostgreSQL time zone syntax");
    assert_eq!(session.setting("timezone").unwrap(), "UTC");
}

#[test]
fn should_reject_invalid_settings_with_22023() {
    // Arrange
    let (cassie, session) = cassie_and_session("invalid_values");

    // Act
    let fixed = cassie.execute_sql(&session, "SET TimeZone='Mars/Olympus_Mons'", vec![]);
    let unknown = cassie.execute_sql(&session, "SET made_up_setting='yes'", vec![]);

    // Assert
    for error in [fixed.unwrap_err(), unknown.unwrap_err()] {
        assert!(error.to_string().contains("parameter"));
    }
}

#[test]
fn should_expose_client_version_functions() {
    // Arrange
    let (cassie, session) = cassie_and_session("version_identity");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT version(), pg_catalog.version(), cassie_version()",
            vec![],
        )
        .expect("version functions");

    // Assert
    assert_eq!(result.rows().len(), 1);
    assert!(
        matches!(&result.rows()[0][0], Value::String(value) if value.starts_with("PostgreSQL 16.0 compatible Cassie"))
    );
    assert_eq!(result.rows()[0][0], result.rows()[0][1]);
    assert_eq!(
        result.rows()[0][2],
        Value::String(env!("CARGO_PKG_VERSION").to_string())
    );
}
