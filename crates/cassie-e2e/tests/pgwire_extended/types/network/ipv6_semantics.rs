//! IPv6 network operators, functions, and arithmetic tracked by #816.

use super::row_columns;
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn assert_error_message(frames: &[(u8, Vec<u8>)], sqlstate: &str, message: &str) {
    assert_eq!(wire::error_code(frames).as_deref(), Some(sqlstate));
    let fields = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("ErrorResponse");
    assert!(
        fields
            .iter()
            .any(|(kind, value)| *kind == 'M' && value == message),
        "expected PostgreSQL message {message:?}, got {fields:?}"
    );
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
}

#[test]
fn should_report_ipv6_inet_cidr_containment_truth_table() {
    // Arrange
    let sql = "SELECT CAST('2001:db8:1::1/128' AS inet) << CAST('2001:db8:1::/64' AS cidr) AS host_in_subnet, CAST('2001:db8:1::/64' AS inet) <<= CAST('2001:db8:1::/64' AS cidr) AS equal_inclusive, CAST('2001:db8::/32' AS inet) >> CAST('2001:db8:1::/64' AS cidr) AS contains_subnet, CAST('2001:db8:1::/64' AS inet) >>= CAST('2001:db8:1::/64' AS cidr) AS reverse_equal_inclusive";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-ipv6-containment", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(16, 1, -1); 4]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}

#[test]
fn should_report_ipv6_inet_cidr_overlap_truth_table() {
    // Arrange
    let sql = "SELECT CAST('2001:db8::1/64' AS inet) && CAST('2001:db8::/32' AS cidr) AS overlaps, CAST('2001:db8:2::/64' AS inet) && CAST('2001:db8:1::/64' AS cidr) AS disjoint, CAST('192.0.2.1' AS inet) && CAST('2001:db8::/32' AS cidr) AS mixed_family";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-ipv6-overlap", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(16, 1, -1); 3]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_classify_mixed_ipv4_ipv6_network_families() {
    // Arrange
    let sql = "SELECT CAST('192.0.2.1' AS inet) << CAST('2001:db8::/32' AS cidr) AS ipv4_in_ipv6, CAST('2001:db8::1' AS inet) << CAST('192.0.2.0/24' AS cidr) AS ipv6_in_ipv4, inet_same_family(CAST('192.0.2.1' AS inet), CAST('2001:db8::1' AS inet)) AS same_family";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-mixed-ip-family", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(16, 1, -1); 3]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("f".to_owned()),
            Some("f".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_report_ipv6_mask_function_results_over_pgwire() {
    // Arrange
    let sql = "SELECT host(CAST('2001:db8::1/64' AS inet)) AS host, family(CAST('2001:db8::1/64' AS inet)) AS family, masklen(CAST('2001:db8::1/64' AS inet)) AS mask_length, set_masklen(CAST('2001:db8::1/64' AS inet), 48) AS inet_with_new_mask, network(CAST('2001:db8::1/64' AS inet)) AS network, broadcast(CAST('2001:db8::1/64' AS inet)) AS broadcast, hostmask(CAST('2001:db8::1/64' AS inet)) AS hostmask, netmask(CAST('2001:db8::1/64' AS inet)) AS netmask, set_masklen(CAST('2001:db8:1::/64' AS cidr), 48) AS cidr_with_new_mask";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-ipv6-mask-functions", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![
            (25, -1, -1),
            (23, 4, -1),
            (23, 4, -1),
            (869, -1, -1),
            (650, -1, -1),
            (869, -1, -1),
            (869, -1, -1),
            (869, -1, -1),
            (650, -1, -1),
        ]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("2001:db8::1".to_owned()),
            Some("6".to_owned()),
            Some("64".to_owned()),
            Some("2001:db8::1/48".to_owned()),
            Some("2001:db8::/64".to_owned()),
            Some("2001:db8::ffff:ffff:ffff:ffff/64".to_owned()),
            Some("::ffff:ffff:ffff:ffff".to_owned()),
            Some("ffff:ffff:ffff:ffff::".to_owned()),
            Some("2001:db8:1::/48".to_owned()),
        ]]
    );
}

#[test]
fn should_reject_ipv6_mask_length_above_128_with_postgresql_sqlstate() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT set_masklen(CAST('2001:db8::1' AS inet), 129)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
    ];

    // Act
    let batches = run_script("postgresql18-ipv6-mask-length-overflow", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_error_message(&batches[0], "22023", "invalid mask length: 129");
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_merge_ipv6_networks_to_the_smallest_common_cidr() {
    // Arrange
    let sql = "SELECT inet_merge(CAST('2001:db8:1::/64' AS cidr), CAST('2001:db8:1:1::/64' AS cidr)) AS adjacent_networks, inet_merge(CAST('2001:db8::1/128' AS inet), CAST('2001:db8::3/128' AS inet)) AS host_addresses";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-ipv6-inet-merge", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(650, -1, -1); 2]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("2001:db8:1::/63".to_owned()),
            Some("2001:db8::/126".to_owned()),
        ]]
    );
}

#[test]
fn should_reject_ipv6_inet_merge_across_address_families() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT inet_merge(CAST('192.0.2.1' AS inet), CAST('2001:db8::1' AS inet))",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 43 AS recovery")]),
    ];

    // Act
    let batches = run_script("postgresql18-inet-merge-family-error", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_error_message(
        &batches[0],
        "22023",
        "cannot merge addresses from different families",
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("43".to_owned())]]
    );
}

#[test]
fn should_apply_ipv6_inet_arithmetic_with_postgresql_result_types() {
    // Arrange
    let sql = "SELECT CAST('2001:db8::1' AS inet) + 1::bigint AS incremented, 1::bigint + CAST('2001:db8::1' AS inet) AS commuted, CAST('2001:db8::2' AS inet) - 1::bigint AS decremented, CAST('2001:db8::3' AS inet) - CAST('2001:db8::1' AS inet) AS difference";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-ipv6-inet-arithmetic", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(869, -1, -1), (869, -1, -1), (869, -1, -1), (20, 8, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("2001:db8::2".to_owned()),
            Some("2001:db8::2".to_owned()),
            Some("2001:db8::1".to_owned()),
            Some("2".to_owned()),
        ]]
    );
}

#[test]
fn should_reject_ipv6_inet_subtraction_from_another_family() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('2001:db8::1' AS inet) - CAST('192.0.2.1' AS inet)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 44 AS recovery")]),
    ];

    // Act
    let batches = run_script("postgresql18-inet-subtract-family-error", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_error_message(
        &batches[0],
        "22023",
        "cannot subtract inet values of different sizes",
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("44".to_owned())]]
    );
}

#[test]
fn should_report_ipv6_address_offset_overflow_with_numeric_range_sqlstate() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff' AS inet) + 1::bigint",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
    ];

    // Act
    let batches = run_script("postgresql18-ipv6-inet-offset-overflow", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_error_message(&batches[0], "22003", "result is out of range");
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("45".to_owned())]]
    );
}
