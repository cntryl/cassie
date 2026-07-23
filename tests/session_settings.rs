use cassie::app::Cassie;
use cassie::types::Value;
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-session-settings-{label}-{}",
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

fn cassie_and_session(label: &str) -> (Cassie, cassie::CassieSession, String) {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let session = cassie.create_session("postgres", Some("postgres".to_string()));
    (cassie, session, path)
}

#[test]
fn should_use_one_registry_for_setting_reads() {
    // Arrange
    let (cassie, session, path) = cassie_and_session("shared_registry");

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
        shown.rows[0][0],
        Value::String("DBeaver 26.1.3".to_string())
    );
    assert_eq!(
        current.rows[0][0],
        Value::String("DBeaver 26.1.3".to_string())
    );
    assert_eq!(catalog.rows, current.rows);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_exact_pgadmin_initialization_settings() {
    // Arrange
    let (cassie, session, path) = cassie_and_session("pgadmin_init");

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
        configured.rows,
        vec![vec![Value::String("hex".to_string())]]
    );
    assert_eq!(session.setting("datestyle").unwrap(), "ISO, MDY");
    assert_eq!(session.setting("client_min_messages").unwrap(), "notice");
    assert_eq!(session.setting("client_encoding").unwrap(), "UTF8");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_invalid_settings_with_22023() {
    // Arrange
    let (cassie, session, path) = cassie_and_session("invalid_values");

    // Act
    let fixed = cassie.execute_sql(&session, "SET TimeZone='America/New_York'", vec![]);
    let unknown = cassie.execute_sql(&session, "SET made_up_setting='yes'", vec![]);

    // Assert
    for error in [fixed.unwrap_err(), unknown.unwrap_err()] {
        assert!(error.to_string().contains("parameter"));
    }
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_expose_client_version_functions() {
    // Arrange
    let (cassie, session, path) = cassie_and_session("version_identity");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT version(), pg_catalog.version(), cassie_version()",
            vec![],
        )
        .expect("version functions");

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert!(
        matches!(&result.rows[0][0], Value::String(value) if value.starts_with("PostgreSQL 16.0 compatible Cassie"))
    );
    assert_eq!(result.rows[0][0], result.rows[0][1]);
    assert_eq!(
        result.rows[0][2],
        Value::String(env!("CARGO_PKG_VERSION").to_string())
    );
    let _ = std::fs::remove_dir_all(path);
}
