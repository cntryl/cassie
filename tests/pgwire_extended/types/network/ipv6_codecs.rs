//! IPv6 network function output and binary wire contracts tracked by #816.

use super::{binary_row, row_columns};
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_apply_ipv6_inet_bitwise_operators_over_pgwire() {
    // Arrange
    let sql = "SELECT ~CAST('2001:db8::1' AS inet) AS complement, CAST('2001:db8::1' AS inet) & CAST('ffff:ffff:ffff:ffff::' AS inet) AS masked, CAST('2001:db8::1' AS inet) | CAST('::f' AS inet) AS merged_bits";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-ipv6-inet-bitwise", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(869, -1, -1); 3]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("dffe:f247:ffff:ffff:ffff:ffff:ffff:fffe".to_owned()),
            Some("2001:db8::".to_owned()),
            Some("2001:db8::f".to_owned()),
        ]]
    );
}

#[test]
fn should_format_ipv6_abbreviated_text_forms() {
    // Arrange
    let sql = "SELECT abbrev(CAST('2001:0db8:0000:0000:0000:ff00:0042:8329/64' AS inet)) AS abbreviated_inet, text(CAST('2001:0db8:0000:0000:0000:ff00:0042:8329/64' AS inet)) AS full_inet, abbrev(CAST('2001:db8::/32' AS cidr)) AS abbreviated_cidr, text(CAST('2001:db8::/32' AS cidr)) AS full_cidr";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-ipv6-abbrev-text", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(25, -1, -1); 4]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("2001:db8::ff00:42:8329/64".to_owned()),
            Some("2001:db8::ff00:42:8329/64".to_owned()),
            Some("2001:db8/32".to_owned()),
            Some("2001:db8::/32".to_owned()),
        ]]
    );
}

#[test]
fn should_round_trip_ipv6_network_values_in_binary_format() {
    // Arrange
    let statement = "ipv6_network_binary_values";
    let portal = "ipv6_network_binary_values_portal";
    let inet_value = [
        3, 64, 0, 16, 0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
    ];
    let cidr_value = [
        3, 32, 1, 16, 0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1 AS address, $2 AS subnet",
            &[869, 650],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                portal,
                statement,
                &[1],
                &[Some(inet_value.as_slice()), Some(cidr_value.as_slice())],
                &[1],
            ),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-ipv6-network-binary-round-trip", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let parameter_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("network ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![869, 650]
    );

    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_columns(result)
            .iter()
            .map(|column| (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code
            ))
            .collect::<Vec<_>>(),
        vec![(869, -1, -1, 1), (650, -1, -1, 1)]
    );
    assert_eq!(
        binary_row(result),
        vec![inet_value.to_vec(), cidr_value.to_vec()]
    );
}

#[test]
fn should_reject_ipv6_cidr_text_host_bits_with_postgresql_sqlstate() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('2001:db8::1/64' AS cidr) AS invalid_subnet",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 46 AS recovery")]),
    ];

    // Act
    let batches = run_script("postgresql18-ipv6-cidr-text-host-bits", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    let rejected = &batches[0];
    assert_eq!(
        wire::error_code(rejected).as_deref(),
        Some("22P02"),
        "CIDR text-input ErrorResponse fields: {:?}",
        rejected
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let error = rejected
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("invalid CIDR text ErrorResponse");
    assert!(wire::parse_error_fields(&error.1)
        .iter()
        .any(|(kind, message)| { *kind == 'M' && message.to_lowercase().contains("cidr") }));
    assert!(
        !rejected
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
        "invalid CIDR text must not produce a query result"
    );
    let recovered = &batches[1];
    assert_eq!(wire::error_code(recovered), None);
    assert_eq!(
        wire::data_rows(recovered),
        vec![vec![Some("46".to_owned())]]
    );
}
