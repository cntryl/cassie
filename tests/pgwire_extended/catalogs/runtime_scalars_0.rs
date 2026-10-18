//! Native descriptors with truthful Cassie runtime-profile values.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_report_pg_is_in_recovery_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_is_in_recovery() AS output";
    // Act
    let batches = planning::pruned("runtime-pg_is_in_recovery_bare", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_is_in_recovery_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_is_in_recovery() AS output";
    // Act
    let batches = planning::pruned("runtime-pg_is_in_recovery_qualified", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_is_in_recovery_where_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_is_in_recovery() AS output WHERE NOT pg_is_in_recovery()";
    // Act
    let batches = planning::pruned("runtime-pg_is_in_recovery_where", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_is_in_recovery_cte_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "WITH q(v) AS (SELECT pg_is_in_recovery()) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("runtime-pg_is_in_recovery_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_is_in_recovery_case_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT CASE WHEN pg_is_in_recovery() THEN TRUE ELSE FALSE END AS output";
    // Act
    let batches = planning::pruned("runtime-pg_is_in_recovery_case", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_jit_available_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_jit_available() AS output";
    // Act
    let batches = planning::pruned("runtime-pg_jit_available_bare", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_jit_available_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_jit_available() AS output";
    // Act
    let batches = planning::pruned("runtime-pg_jit_available_qualified", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_jit_available_where_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_jit_available() AS output WHERE NOT pg_jit_available()";
    // Act
    let batches = planning::pruned("runtime-pg_jit_available_where", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_jit_available_cte_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "WITH q(v) AS (SELECT pg_jit_available()) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("runtime-pg_jit_available_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_jit_available_case_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT CASE WHEN pg_jit_available() THEN TRUE ELSE FALSE END AS output";
    // Act
    let batches = planning::pruned("runtime-pg_jit_available_case", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_numa_available_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_numa_available() AS output";
    // Act
    let batches = planning::pruned("runtime-pg_numa_available_bare", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_numa_available_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_numa_available() AS output";
    // Act
    let batches = planning::pruned("runtime-pg_numa_available_qualified", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_numa_available_where_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_numa_available() AS output WHERE NOT pg_numa_available()";
    // Act
    let batches = planning::pruned("runtime-pg_numa_available_where", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_numa_available_cte_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "WITH q(v) AS (SELECT pg_numa_available()) SELECT v AS output FROM q";
    // Act
    let batches = planning::pruned("runtime-pg_numa_available_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_pg_numa_available_case_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT CASE WHEN pg_numa_available() THEN TRUE ELSE FALSE END AS output";
    // Act
    let batches = planning::pruned("runtime-pg_numa_available_case", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 16, 1, -1),
        [&[&[0, 1, 0, 0, 0, 1, 102]], &[&[0, 1, 0, 0, 0, 1, 0]]],
    );
}

#[test]
fn should_report_compression_0_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression('plain'::text) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_0_bare", sql);
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
fn should_report_compression_0_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression('plain'::text) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_0_qualified", sql);
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
fn should_report_compression_1_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression('plain') AS output";
    // Act
    let batches = planning::pruned("runtime-compression_1_bare", sql);
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
fn should_report_compression_1_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression('plain') AS output";
    // Act
    let batches = planning::pruned("runtime-compression_1_qualified", sql);
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
fn should_report_compression_2_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression(NULL) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_2_bare", sql);
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
fn should_report_compression_2_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression(NULL) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_2_qualified", sql);
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
fn should_report_compression_3_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression(NULL::text) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_3_bare", sql);
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
fn should_report_compression_3_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression(NULL::text) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_3_qualified", sql);
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
fn should_report_compression_4_bare_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_column_compression(1::int) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_4_bare", sql);
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
fn should_report_compression_4_qualified_given_current_runtime_capabilities_when_wire_formats_are_crossed(
) {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_compression(1::int) AS output";
    // Act
    let batches = planning::pruned("runtime-compression_4_qualified", sql);
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
