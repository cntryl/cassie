//! PostgreSQL 18 inet, cidr, macaddr, and macaddr8 behavior tracked by #816.

use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn query_rows(fixture: &SqlFixture, sql: &str) -> Vec<Vec<Value>> {
    fixture
        .execute(sql)
        .unwrap_or_else(|error| panic!("network SQL failed: {sql}: {error}"))
        .rows()
        .to_vec()
}

#[test]
fn should_format_ipv4_inet_text_forms() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-ipv4-output", &[]);
    let sql = "SELECT host(CAST('192.168.1.5/24' AS inet)), text(CAST('192.168.1.5/24' AS inet))";

    // Act
    let rows = query_rows(&fixture, sql);

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("192.168.1.5".to_owned()),
            Value::String("192.168.1.5/24".to_owned()),
        ]]
    );
}

#[test]
fn should_canonicalize_ipv6_inet_output() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-ipv6-output", &[]);
    let sql = "SELECT host(CAST('2001:0db8:0000:0000:0000:ff00:0042:8329' AS inet))";

    // Act
    let rows = query_rows(&fixture, sql);

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("2001:db8::ff00:42:8329".to_owned())]]
    );
}

#[test]
fn should_assign_full_host_prefix_when_inet_prefix_is_omitted() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-default-prefix", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.0.2.9' AS inet) = CAST('192.0.2.9/32' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_keep_inet_host_bits_as_part_of_value_identity() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-host-bits", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.5/24' AS inet) <> CAST('192.168.1.0/24' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_sort_ipv4_before_ipv6_inet_values() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-family-order", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('255.255.255.255' AS inet) < CAST('::1' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_order_inet_prefix_length_after_equal_network_bits() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-prefix-order", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.0.2.1/24' AS inet) < CAST('192.0.2.1/25' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_ignore_inet_host_bits_when_testing_subnet_containment() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-contained-host", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.5/24' AS inet) <<= CAST('192.168.1.0/24' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_strict_subnet_containment() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-strict-containment", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.0/25' AS inet) << CAST('192.168.1.0/24' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_subnet_containment_or_equality() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-containment-equality", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.0/24' AS inet) <<= CAST('192.168.1.0/24' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_reverse_subnet_containment() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-reverse-containment", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.0/24' AS inet) >> CAST('192.168.1.0/25' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_reverse_subnet_containment_or_equality() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-reverse-containment-equality", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.0/24' AS inet) >>= CAST('192.168.1.0/24' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_overlapping_inet_subnets() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-subnet-overlap", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.0/24' AS inet) && CAST('192.168.1.80/28' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_ipv4_prefix_family_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-mask-family", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT masklen(CAST('192.168.1.5/24' AS inet)), family(CAST('192.168.1.5/24' AS inet))",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(24), Value::Int64(4)]]);
}

#[test]
fn should_report_ipv6_prefix_family_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-ipv6-family", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT masklen(CAST('2001:db8::1' AS inet)), family(CAST('2001:db8::1' AS inet))",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(128), Value::Int64(6)]]);
}

#[test]
fn should_calculate_ipv4_network_address() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-network", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT network(CAST('192.168.1.5/24' AS inet)) = CAST('192.168.1.0/24' AS cidr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_calculate_ipv4_broadcast_address() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-broadcast", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT broadcast(CAST('192.168.1.5/24' AS inet)) = CAST('192.168.1.255/24' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_calculate_ipv4_netmask() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-netmask", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT netmask(CAST('192.168.1.5/24' AS inet)) = CAST('255.255.255.0' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_calculate_ipv4_hostmask() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-hostmask", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT hostmask(CAST('192.168.23.20/30' AS inet)) = CAST('0.0.0.3' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_abbreviate_inet_without_its_host_prefix() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-abbrev", &[]);

    // Act
    let rows = query_rows(&fixture, "SELECT abbrev(CAST('10.1.0.0/32' AS inet))");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("10.1.0.0".to_owned())]]);
}

#[test]
fn should_abbreviate_cidr_by_dropping_zero_octets() {
    // Arrange
    let fixture = sql_fixture("issue-816-cidr-abbrev", &[]);

    // Act
    let rows = query_rows(&fixture, "SELECT abbrev(CAST('192.168.0.0/16' AS cidr))");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("192.168/16".to_owned())]]);
}

#[test]
fn should_normalize_abbreviated_cidr_input() {
    // Arrange
    let fixture = sql_fixture("issue-816-cidr-abbreviated-input", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1/24' AS cidr) = CAST('192.168.1.0/24' AS cidr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_infer_classful_cidr_prefix_when_omitted() {
    // Arrange
    let fixture = sql_fixture("issue-816-cidr-classful-prefix", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('10' AS cidr) = CAST('10.0.0.0/8' AS cidr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_zero_host_bits_when_casting_inet_to_cidr() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-cidr-cast", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST(CAST('192.168.1.5/24' AS inet) AS cidr) = CAST('192.168.1.0/24' AS cidr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_reject_cidr_input_with_host_bits_set() {
    // Arrange
    let fixture = sql_fixture("issue-816-cidr-host-bits-invalid", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('192.168.1.5/24' AS cidr)");

    // Assert
    assert!(result.is_err(), "cidr input with host bits must fail");
}

#[test]
fn should_reject_ipv4_inet_prefix_above_its_address_width() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-mask-too-wide", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('192.0.2.1/33' AS inet)");

    // Assert
    assert!(result.is_err(), "IPv4 inet prefix /33 must fail");
}

#[test]
fn should_reject_ipv6_inet_prefix_above_its_address_width() {
    // Arrange
    let fixture = sql_fixture("issue-816-ipv6-mask-too-wide", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('2001:db8::1/129' AS inet)");

    // Assert
    assert!(result.is_err(), "IPv6 inet prefix /129 must fail");
}

#[test]
fn should_reject_invalid_inet_address_text() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-invalid-address", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('192.0.2.256' AS inet)");

    // Assert
    assert!(result.is_err(), "invalid IPv4 octet must fail");
}

#[test]
fn should_reject_set_masklen_outside_the_address_family_range() {
    // Arrange
    let fixture = sql_fixture("issue-816-set-masklen-range", &[]);

    // Act
    let result = fixture.execute("SELECT set_masklen(CAST('192.0.2.1' AS inet), 33)");

    // Assert
    assert!(result.is_err(), "IPv4 mask length 33 must fail");
}

#[test]
fn should_set_inet_mask_length_without_changing_host_bits() {
    // Arrange
    let fixture = sql_fixture("issue-816-set-inet-masklen", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT set_masklen(CAST('192.168.1.5/24' AS inet), 16) = CAST('192.168.1.5/16' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_clear_new_host_bits_when_setting_cidr_mask_length() {
    // Arrange
    let fixture = sql_fixture("issue-816-set-cidr-masklen", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT set_masklen(CAST('192.168.1.0/24' AS cidr), 16) = CAST('192.168.0.0/16' AS cidr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_detect_matching_inet_address_families() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-same-family", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT inet_same_family(CAST('192.0.2.1' AS inet), CAST('198.51.100.1' AS cidr))",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_merge_ipv4_networks_to_the_smallest_common_cidr() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-merge", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT inet_merge(CAST('192.168.1.5/24' AS inet), CAST('192.168.2.5/24' AS inet)) = CAST('192.168.0.0/22' AS cidr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_add_ipv4_address_offset() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-address-offset", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.6' AS inet) + 25 = CAST('192.168.1.31' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_add_ipv4_address_offset_with_integer_on_the_left() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-offset-left", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT 25 + CAST('192.168.1.6' AS inet) = CAST('192.168.1.31' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_subtract_ipv4_address_offset() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-offset-subtract", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.43' AS inet) - 36 = CAST('192.168.1.7' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_return_ipv4_address_difference_as_bigint() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-address-difference", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.43' AS inet) - CAST('192.168.1.19' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(24)]]);
}

#[test]
fn should_apply_ipv4_inet_bitwise_not() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-bitwise-not", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT ~ CAST('192.168.1.6' AS inet) = CAST('63.87.254.249' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_apply_ipv4_inet_bitwise_and() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-bitwise-and", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.6' AS inet) & CAST('0.0.0.255' AS inet) = CAST('0.0.0.6' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_apply_ipv4_inet_bitwise_or() {
    // Arrange
    let fixture = sql_fixture("issue-816-inet-bitwise-or", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('192.168.1.6' AS inet) | CAST('0.0.0.255' AS inet) = CAST('192.168.1.255' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_accept_equivalent_macaddr_spellings() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-input-spellings", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('08:00:2b:01:02:03' AS macaddr) = CAST('0800.2b01.0203' AS macaddr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_format_macaddr_in_canonical_colon_form() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-output", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST(CAST('08002b010203' AS macaddr) AS text)",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("08:00:2b:01:02:03".to_owned())]]
    );
}

#[test]
fn should_accept_macaddr8_six_byte_input_as_eui64() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr8-eui48-input", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('08:00:2b:01:02:03' AS macaddr8) = CAST('08:00:2b:ff:fe:01:02:03' AS macaddr8)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_accept_macaddr8_dotted_group_input() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr8-dotted-input", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('0800.2b01.0203.0405' AS macaddr8) = CAST('08:00:2b:01:02:03:04:05' AS macaddr8)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_format_macaddr8_in_canonical_colon_form() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr8-output", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST(CAST('08002b0102030405' AS macaddr8) AS text)",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("08:00:2b:01:02:03:04:05".to_owned())]]
    );
}

#[test]
fn should_insert_fffe_when_promoting_macaddr_to_macaddr8() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr8-insert-fffe", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST(CAST('08:00:2b:01:02:03' AS macaddr8) AS text)",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("08:00:2b:ff:fe:01:02:03".to_owned())]]
    );
}

#[test]
fn should_set_the_modified_eui64_seventh_bit() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr8-set7bit", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT macaddr8_set7bit(CAST('08:00:2b:01:02:03' AS macaddr8)) = CAST('0a:00:2b:ff:fe:01:02:03' AS macaddr8)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_truncate_macaddr_host_bytes() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-trunc", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT trunc(CAST('12:34:56:78:90:ab' AS macaddr)) = CAST('12:34:56:00:00:00' AS macaddr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_truncate_macaddr8_host_bytes() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr8-trunc", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT trunc(CAST('12:34:56:78:90:ab:cd:ef' AS macaddr8)) = CAST('12:34:56:00:00:00:00:00' AS macaddr8)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_apply_macaddr_bitwise_not() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-bitwise-not", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT ~ CAST('12:34:56:78:90:ab' AS macaddr) = CAST('ed:cb:a9:87:6f:54' AS macaddr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_apply_macaddr_bitwise_and() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-bitwise-and", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('08:00:2b:01:02:03' AS macaddr) & CAST('ff:ff:00:ff:00:ff' AS macaddr) = CAST('08:00:00:01:00:03' AS macaddr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_apply_macaddr_bitwise_or() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-bitwise-or", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('08:00:2b:01:02:03' AS macaddr) | CAST('00:ff:00:00:ff:00' AS macaddr) = CAST('08:ff:2b:01:ff:03' AS macaddr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_order_macaddr_values_by_address_bytes() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-order", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST('08:00:2b:01:02:03' AS macaddr) < CAST('08:00:2b:01:02:04' AS macaddr)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_reject_invalid_macaddr_text() {
    // Arrange
    let fixture = sql_fixture("issue-816-macaddr-invalid", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('08:00:2b:01:02:zz' AS macaddr)");

    // Assert
    assert!(result.is_err(), "invalid macaddr octet must fail");
}

#[test]
fn should_propagate_null_through_network_operations() {
    // Arrange
    let fixture = sql_fixture("issue-816-network-null-propagation", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT host(CAST(NULL AS inet)) IS NULL, CAST(NULL AS inet) << CAST('192.0.2.0/24' AS inet) IS NULL, macaddr8_set7bit(CAST(NULL AS macaddr8)) IS NULL",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true); 3]]);
}

#[test]
fn should_preserve_network_types_through_table_storage() {
    // Arrange
    let fixture = sql_fixture("issue-816-network-storage-visibility", &[]);
    let create = fixture.execute(
        "CREATE TABLE endpoint_addresses (id INTEGER, ip inet, subnet cidr, nic macaddr, eui macaddr8)",
    );
    create.expect("network type columns should be accepted");

    // Act
    fixture
        .execute("INSERT INTO endpoint_addresses VALUES (1, '192.0.2.9/24', '192.0.2.0/24', '08:00:2b:01:02:03', '08:00:2b:01:02:03:04:05')")
        .expect("network type values should be stored");
    let rows = query_rows(
        &fixture,
        "SELECT ip = CAST('192.0.2.9/24' AS inet), subnet = CAST('192.0.2.0/24' AS cidr), nic = CAST('08:00:2b:01:02:03' AS macaddr), eui = CAST('08:00:2b:01:02:03:04:05' AS macaddr8) FROM endpoint_addresses WHERE id = 1",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true); 4]]);
}

#[test]
fn should_compare_network_values_in_a_null_safe_predicate() {
    // Arrange
    let fixture = sql_fixture("issue-816-network-null-safe-comparison", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST(NULL AS inet) IS NOT DISTINCT FROM CAST(NULL AS inet), CAST('192.0.2.1' AS inet) IS DISTINCT FROM CAST('192.0.2.2' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true), Value::Bool(true)]]);
}

#[test]
fn should_return_null_for_network_comparisons_with_null_operands() {
    // Arrange
    let fixture = sql_fixture("issue-816-network-null-comparison", &[]);

    // Act
    let rows = query_rows(
        &fixture,
        "SELECT CAST(NULL AS inet) = CAST('192.0.2.1' AS inet)",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Null]]);
}
