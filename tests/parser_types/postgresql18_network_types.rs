//! PostgreSQL 18 network and hardware-address grammar tracked by #816.

#[test]
fn should_parse_inet_type_cast() {
    // Arrange
    let sql = "SELECT CAST('192.0.2.9/24' AS inet)";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "inet cast should parse: {parsed:?}");
}

#[test]
fn should_parse_cidr_type_cast() {
    // Arrange
    let sql = "SELECT CAST('192.0.2.0/24' AS cidr)";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "cidr cast should parse: {parsed:?}");
}

#[test]
fn should_parse_macaddr_type_cast() {
    // Arrange
    let sql = "SELECT CAST('08:00:2b:01:02:03' AS macaddr)";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "macaddr cast should parse: {parsed:?}");
}

#[test]
fn should_parse_macaddr8_type_cast() {
    // Arrange
    let sql = "SELECT CAST('08:00:2b:01:02:03:04:05' AS macaddr8)";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "macaddr8 cast should parse: {parsed:?}");
}

#[test]
fn should_parse_inet_subnet_containment_operator() {
    // Arrange
    let sql = "SELECT inet '192.168.1.5' << inet '192.168.1.0/24'";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "inet << should parse: {parsed:?}");
}

#[test]
fn should_parse_inet_subnet_overlap_operator() {
    // Arrange
    let sql = "SELECT inet '192.168.1.0/24' && inet '192.168.1.80/28'";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "inet && should parse: {parsed:?}");
}

#[test]
fn should_parse_inet_address_arithmetic() {
    // Arrange
    let sql = "SELECT inet '192.168.1.6' + 25";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(
        parsed.is_ok(),
        "inet address arithmetic should parse: {parsed:?}"
    );
}

#[test]
fn should_parse_macaddr_bitwise_not_operator() {
    // Arrange
    let sql = "SELECT ~ macaddr '12:34:56:78:90:ab'";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(
        parsed.is_ok(),
        "macaddr bitwise NOT should parse: {parsed:?}"
    );
}
