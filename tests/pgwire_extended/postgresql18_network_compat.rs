//! PostgreSQL 18 network and hardware-address wire contract tracked by #816.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn row_columns(frames: &[(u8, Vec<u8>)]) -> Vec<wire::RowDescription> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
}

fn binary_row(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<u8>> {
    let payload = &frames
        .iter()
        .find(|(tag, _)| *tag == b'D')
        .expect("DataRow")
        .1;
    let mut cursor = 0_usize;
    let field_count =
        i16::from_be_bytes(payload[cursor..cursor + 2].try_into().expect("field count"));
    cursor += 2;
    let mut fields = Vec::new();
    for _ in 0..field_count {
        let length = i32::from_be_bytes(
            payload[cursor..cursor + 4]
                .try_into()
                .expect("field length"),
        );
        cursor += 4;
        assert!(length >= 0, "network output fields should not be NULL");
        let end = cursor + usize::try_from(length).expect("nonnegative field length");
        fields.push(payload[cursor..end].to_vec());
        cursor = end;
    }
    assert_eq!(cursor, payload.len(), "DataRow has no trailing bytes");
    fields
}

#[test]
fn should_describe_network_types_with_postgresql_oids() {
    // Arrange
    let sql = "SELECT CAST('192.0.2.9/24' AS inet) AS ipv4, CAST('2001:db8::1/64' AS inet) AS ipv6, CAST('192.0.2.0/24' AS cidr) AS subnet, CAST('08:00:2b:01:02:03' AS macaddr) AS mac, CAST('08:00:2b:01:02:03:04:05' AS macaddr8) AS eui64";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-network-oids", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let columns = row_columns(&batches[0]);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![869, 869, 650, 829, 774]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_size)
            .collect::<Vec<_>>(),
        vec![-1, -1, -1, 6, 8]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("192.0.2.9/24".to_owned()),
            Some("2001:db8::1/64".to_owned()),
            Some("192.0.2.0/24".to_owned()),
            Some("08:00:2b:01:02:03".to_owned()),
            Some("08:00:2b:01:02:03:04:05".to_owned()),
        ]]
    );
}

#[test]
fn should_describe_network_array_types_with_postgresql_oids() {
    // Arrange
    let sql = "SELECT CAST(ARRAY[CAST('192.0.2.1' AS inet)] AS inet[]) AS ips, CAST(ARRAY[CAST('192.0.2.0/24' AS cidr)] AS cidr[]) AS subnets, CAST(ARRAY[CAST('08:00:2b:01:02:03' AS macaddr)] AS macaddr[]) AS macs, CAST(ARRAY[CAST('08:00:2b:01:02:03:04:05' AS macaddr8)] AS macaddr8[]) AS eui64s";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-network-array-oids", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        row_columns(&batches[0])
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1041, 651, 1040, 775]
    );
}

#[test]
fn should_send_network_values_in_postgresql_binary_format() {
    // Arrange
    let sql = "SELECT CAST('192.0.2.9/24' AS inet) AS ipv4, CAST('2001:db8::1/64' AS inet) AS ipv6, CAST('192.0.2.0/24' AS cidr) AS subnet, CAST('08:00:2b:01:02:03' AS macaddr) AS mac, CAST('08:00:2b:01:02:03:04:05' AS macaddr8) AS eui64";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("network_binary_output", sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "network_binary_output_portal",
                "network_binary_output",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("network_binary_output_portal"),
            wire::execute_frame("network_binary_output_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-network-binary-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let columns = row_columns(&batches[1]);
    assert!(columns.iter().all(|column| column.format_code == 1));
    assert_eq!(
        binary_row(&batches[1]),
        vec![
            vec![2, 24, 0, 4, 192, 0, 2, 9],
            vec![3, 64, 0, 16, 0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,],
            vec![2, 24, 1, 4, 192, 0, 2, 0],
            vec![0x08, 0x00, 0x2b, 0x01, 0x02, 0x03],
            vec![0x08, 0x00, 0x2b, 0x01, 0x02, 0x03, 0x04, 0x05],
        ]
    );
}

#[test]
fn should_decode_text_parameters_for_address_types() {
    // Arrange
    let sql = "SELECT $1 AS ipv4, $2 AS subnet, $3 AS mac, $4 AS eui64";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "network_text_parameters",
            sql,
            &[869, 650, 829, 774],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("network_text_parameters"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "network_text_portal",
                "network_text_parameters",
                &[
                    "192.0.2.9/24",
                    "192.0.2.0/24",
                    "0800.2b01.0203",
                    "0800.2b01.0203.0405",
                ],
            ),
            wire::describe_portal_frame("network_text_portal"),
            wire::execute_frame("network_text_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-network-text-parameters", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::parse_parameter_description(
            &batches[1]
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("ParameterDescription")
                .1
        ),
        vec![869, 650, 829, 774]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        row_columns(&batches[2])
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![869, 650, 829, 774]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("192.0.2.9/24".to_owned()),
            Some("192.0.2.0/24".to_owned()),
            Some("08:00:2b:01:02:03".to_owned()),
            Some("08:00:2b:01:02:03:04:05".to_owned()),
        ]]
    );
}

#[test]
fn should_decode_binary_parameters_for_address_types() {
    // Arrange
    let sql = "SELECT $1 AS ipv4, $2 AS subnet, $3 AS mac, $4 AS eui64";
    let ipv4 = [2, 24, 0, 4, 192, 0, 2, 9];
    let subnet = [2, 24, 1, 4, 192, 0, 2, 0];
    let mac = [0x08, 0x00, 0x2b, 0x01, 0x02, 0x03];
    let eui64 = [0x08, 0x00, 0x2b, 0x01, 0x02, 0x03, 0x04, 0x05];
    let parameters = [
        Some(ipv4.as_slice()),
        Some(subnet.as_slice()),
        Some(mac.as_slice()),
        Some(eui64.as_slice()),
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "network_binary_parameters",
            sql,
            &[869, 650, 829, 774],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "network_binary_parameter_portal",
                "network_binary_parameters",
                &[1],
                &parameters,
                &[0],
            ),
            wire::describe_portal_frame("network_binary_parameter_portal"),
            wire::execute_frame("network_binary_parameter_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-network-binary-parameters", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![
            Some("192.0.2.9/24".to_owned()),
            Some("192.0.2.0/24".to_owned()),
            Some("08:00:2b:01:02:03".to_owned()),
            Some("08:00:2b:01:02:03:04:05".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_address_oids_for_null_parameters() {
    // Arrange
    let sql = "SELECT $1 AS ipv4, $2 AS subnet, $3 AS mac, $4 AS eui64";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "network_null_parameters",
            sql,
            &[869, 650, 829, 774],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "network_null_portal",
                "network_null_parameters",
                &[0],
                &[None, None, None, None],
                &[0],
            ),
            wire::describe_portal_frame("network_null_portal"),
            wire::execute_frame("network_null_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-network-null-parameters", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        row_columns(&batches[1])
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![869, 650, 829, 774]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![None, None, None, None]]
    );
}

#[test]
fn should_reject_invalid_network_binary_payloads_then_recover() {
    // Arrange
    let cases = [
        ("inet_family", 869, vec![1, 32, 0, 4, 192, 0, 2, 1]),
        ("inet_prefix", 869, vec![2, 33, 0, 4, 192, 0, 2, 1]),
        ("inet_length", 869, vec![2, 24, 0, 3, 192, 0, 2]),
        ("cidr_host_bits", 650, vec![2, 24, 1, 4, 192, 0, 2, 9]),
    ];

    // Act
    let results = cases.map(|(case_name, type_oid, value)| {
        let steps = vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "malformed_network_parameter",
                "SELECT $1 AS address",
                &[type_oid],
            )),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "malformed_network_portal",
                    "malformed_network_parameter",
                    &[1],
                    &[Some(value.as_slice())],
                    &[0],
                ),
                wire::describe_portal_frame("malformed_network_portal"),
                wire::execute_frame("malformed_network_portal"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
        ];
        let batches = run_script(&format!("postgresql18-{case_name}"), steps);
        (batches, case_name)
    });

    // Assert
    for (batches, case_name) in results {
        assert_eq!(wire::error_code(&batches[0]), None, "Parse: {case_name}");
        assert_eq!(
            wire::error_code(&batches[1]).as_deref(),
            Some("22P03"),
            "binary receive SQLSTATE: {case_name}"
        );
        assert_eq!(wire::error_code(&batches[2]), None, "recovery: {case_name}");
        assert_eq!(
            wire::data_rows(&batches[2]),
            vec![vec![Some("42".to_owned())]],
            "recovery result: {case_name}"
        );
    }
}

#[test]
fn should_report_postgresql_sqlstates_for_invalid_network_inputs() {
    // Arrange
    let cases = [
        ("inet_text", "SELECT CAST('192.0.2.256' AS inet)", "22P02"),
        (
            "cidr_host_bits",
            "SELECT CAST('192.0.2.9/24' AS cidr)",
            "22P02",
        ),
        (
            "macaddr_text",
            "SELECT CAST('08:00:2b:01:02:zz' AS macaddr)",
            "22P02",
        ),
        (
            "mask_length",
            "SELECT set_masklen(CAST('192.0.2.1' AS inet), 33)",
            "22023",
        ),
    ];

    // Act
    let results = cases.map(|(case_name, sql, expected_code)| {
        let batches = run_script(
            &format!("postgresql18-network-invalid-{case_name}"),
            vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
        );
        (batches, case_name, expected_code)
    });

    // Assert
    for (batches, case_name, expected_code) in results {
        assert_eq!(
            wire::error_code(&batches[0]).as_deref(),
            Some(expected_code),
            "SQLSTATE: {case_name}"
        );
    }
}
