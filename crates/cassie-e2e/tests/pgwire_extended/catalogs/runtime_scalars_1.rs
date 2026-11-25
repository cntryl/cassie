//! Native descriptors with truthful Cassie runtime-profile values.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_report_compression_5_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression(1::bigint) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_5_bare", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_5_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression(1::bigint) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_5_qualified", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_6_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression(TRUE) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_6_bare", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_6_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression(TRUE) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_6_qualified", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_7_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression(1.5::float8) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_7_bare", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_7_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression(1.5::float8) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_7_qualified", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_8_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression('abc'::varchar(5)) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_8_bare", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_8_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression('abc'::varchar(5)) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_8_qualified", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_9_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression('a'::char(4)) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_9_bare", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_9_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression('a'::char(4)) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_9_qualified", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_10_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression('\\x00ff'::bytea) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_10_bare", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_compression_10_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression('\\x00ff'::bytea) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_10_qualified", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[&[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_report_unicode_0_given_current_runtime_capabilities_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT unicode_version() IS NOT NULL AS output";
    // Act
    let batches = planning::pruned("runtime-unicode_0", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}

#[test]
fn should_report_unicode_1_given_current_runtime_capabilities_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_catalog.unicode_version() <> '' AS output";
    // Act
    let batches = planning::pruned("runtime-unicode_1", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 116]], &[&[0, 1, 0, 0, 0, 1, 1]]],
    );
}
