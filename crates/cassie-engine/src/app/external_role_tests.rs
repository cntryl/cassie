use super::{Cassie, CassieError};
use crate::config::CassieRuntimeConfig;

fn fixture() -> Cassie {
    let config = CassieRuntimeConfig {
        password: "external-role-fixture-password".to_owned(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory instance");
    cassie.startup().expect("start Cassie");
    cassie
        .create_role("external_reader", false, None, false)
        .expect("create reader without login");
    cassie
}

#[test]
fn should_enforce_external_database_grants_independently_of_the_role_connect_grants() {
    // Arrange
    let cassie = fixture();
    cassie
        .create_logical_database("allowed", false)
        .expect("create allowed");
    let authority = cassie
        .capture_external_role("external_reader", &["db:allowed".to_owned()], u64::MAX)
        .expect("capture authority");
    let principal = cassie.external_principal(&authority).expect("principal");

    // Act
    let allowed = cassie
        .network_database_session(&principal.session, Some(&principal.role), "allowed")
        .expect("rebind to token-granted database");
    let result = cassie.execute_sql(&allowed, "SELECT 1", vec![]);
    let default_denial =
        cassie.network_database_session(&allowed, Some(&principal.role), cassie.default_database());
    let mut moved = allowed;
    moved.database = Some(cassie.default_database().to_owned());
    let engine_denial = cassie.execute_sql(&moved, "SELECT 1", vec![]);

    // Assert
    assert!(result.is_ok());
    assert!(matches!(
        default_denial,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert!(matches!(
        engine_denial,
        Err(CassieError::InsufficientPrivilege)
    ));
}

#[test]
fn should_authorize_later_databases_given_the_literal_external_wildcard() {
    // Arrange
    let cassie = fixture();
    let authority = cassie
        .capture_external_role("external_reader", &["db:*".to_owned()], u64::MAX)
        .expect("capture wildcard");
    let principal = cassie.external_principal(&authority).expect("principal");

    // Act
    cassie
        .create_logical_database("later", false)
        .expect("create later");
    let bound = cassie.network_database_session(&principal.session, Some(&principal.role), "later");
    let absent =
        cassie.network_database_session(&principal.session, Some(&principal.role), "absent");

    // Assert
    assert!(bound.is_ok());
    assert!(absent.is_err());
}

#[test]
fn should_reject_a_replacement_role_given_a_previously_captured_external_authority() {
    // Arrange
    let cassie = fixture();
    let authority = cassie
        .capture_external_role("external_reader", &["db:*".to_owned()], u64::MAX)
        .expect("capture original role");

    // Act
    cassie
        .drop_role("external_reader", false)
        .expect("drop role");
    cassie
        .create_role("external_reader", false, None, false)
        .expect("replace role");
    let outcome = cassie.external_principal(&authority);

    // Assert
    assert!(matches!(outcome, Err(CassieError::Unauthorized)));
}
