//! PostgreSQL 18 network and hardware-address grammar tracked by #816.
//!
//! Keep the network casts, typed literals, and operators below as SQL-level
//! acceptance cases. The current AST has no network `DataType` or network
//! operator variants, so a structural assertion here would have to erase the
//! identity these cases are intended to cover.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_execute_inet_cast_with_postgres_identity() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-cast", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('192.0.2.9/24' AS inet)");

    // Assert
    let result = result.expect("inet cast executes");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("192.0.2.9/24".to_string())]]
    );
    assert_eq!(result.columns()[0].type_oid, 869);
}

#[test]
fn should_execute_cidr_cast_with_postgres_identity() {
    // Arrange
    let fixture = sql_fixture("issue-816-cidr-cast", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('192.0.2.0/24' AS cidr)");

    // Assert
    let result = result.expect("cidr cast executes");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("192.0.2.0/24".to_string())]]
    );
    assert_eq!(result.columns()[0].type_oid, 650);
}

#[test]
fn should_execute_macaddr_cast_with_postgres_identity() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-cast", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('08:00:2b:01:02:03' AS macaddr)");

    // Assert
    let result = result.expect("macaddr cast executes");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("08:00:2b:01:02:03".to_string())]]
    );
    assert_eq!(result.columns()[0].type_oid, 829);
}

#[test]
fn should_execute_macaddr8_cast_with_postgres_identity() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr8-cast", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('08:00:2b:01:02:03:04:05' AS macaddr8)");

    // Assert
    let result = result.expect("macaddr8 cast executes");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("08:00:2b:01:02:03:04:05".to_string())]]
    );
    assert_eq!(result.columns()[0].type_oid, 774);
}

#[test]
fn should_evaluate_inet_subnet_containment() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-containment", &[]);

    // Act
    let result = fixture.execute("SELECT inet '192.168.1.5' << inet '192.168.1.0/24'");

    // Assert
    let result = result.expect("inet containment executes");
    assert_eq!(result.rows(), &[vec![Value::Bool(true)]]);
    assert_eq!(result.columns()[0].type_oid, 16);
}

#[test]
fn should_evaluate_inet_subnet_overlap() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-overlap", &[]);

    // Act
    let result = fixture.execute("SELECT inet '192.168.1.0/24' && inet '192.168.1.80/28'");

    // Assert
    let result = result.expect("inet overlap executes");
    assert_eq!(result.rows(), &[vec![Value::Bool(true)]]);
    assert_eq!(result.columns()[0].type_oid, 16);
}

#[test]
fn should_add_an_integer_to_inet_address() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-add", &[]);

    // Act
    let result = fixture.execute("SELECT inet '192.168.1.6' + 25");

    // Assert
    let result = result.expect("inet address addition executes");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("192.168.1.31/32".to_string())]]
    );
    assert_eq!(result.columns()[0].type_oid, 869);
}

#[test]
fn should_invert_macaddr_bits() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-not", &[]);

    // Act
    let result = fixture.execute("SELECT ~ macaddr '12:34:56:78:90:ab'");

    // Assert
    let result = result.expect("macaddr bitwise NOT executes");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("ed:cb:a9:87:6f:54".to_string())]]
    );
    assert_eq!(result.columns()[0].type_oid, 829);
}
