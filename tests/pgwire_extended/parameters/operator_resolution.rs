//! Typed and inferred BYTEA/integer operators retain native result identities.

use crate::support_parameter_operators::{assert_case, run, Case};

#[test]
fn should_resolve_bytea_literal_known_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 || '\\x02'::bytea AS value",
        supplied: &[17],
        resolved: &[17],
        text_inputs: &[&[92, 120, 48, 49]],
        binary_inputs: &[&[1]],
        column: (17, -1, -1),
        text_output: &[92, 120, 48, 49, 48, 50],
        binary_output: &[1, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bytea_literal_unknown_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 || '\\x02'::bytea AS value",
        supplied: &[0],
        resolved: &[17],
        text_inputs: &[&[92, 120, 48, 49]],
        binary_inputs: &[&[1]],
        column: (17, -1, -1),
        text_output: &[92, 120, 48, 49, 48, 50],
        binary_output: &[1, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bytea_casts_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1::bytea || $2::bytea AS value",
        supplied: &[0, 0],
        resolved: &[17, 17],
        text_inputs: &[&[92, 120, 48, 49], &[92, 120, 48, 50]],
        binary_inputs: &[&[1], &[2]],
        column: (17, -1, -1),
        text_output: &[92, 120, 48, 49, 48, 50],
        binary_output: &[1, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bytea_column_known_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT b || $1 AS value FROM operator_inputs",
        supplied: &[17],
        resolved: &[17],
        text_inputs: &[&[92, 120, 48, 50]],
        binary_inputs: &[&[2]],
        column: (17, -1, -1),
        text_output: &[92, 120, 48, 49, 48, 50],
        binary_output: &[1, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bytea_column_unknown_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT b || $1 AS value FROM operator_inputs",
        supplied: &[0],
        resolved: &[17],
        text_inputs: &[&[92, 120, 48, 50]],
        binary_inputs: &[&[2]],
        column: (17, -1, -1),
        text_output: &[92, 120, 48, 49, 48, 50],
        binary_output: &[1, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bytea_column_cast_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT b || $1::bytea AS value FROM operator_inputs",
        supplied: &[0],
        resolved: &[17],
        text_inputs: &[&[92, 120, 48, 50]],
        binary_inputs: &[&[2]],
        column: (17, -1, -1),
        text_output: &[92, 120, 48, 49, 48, 50],
        binary_output: &[1, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bytea_compare_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT 2 AS value WHERE $1 || $2 = '\\x0102'::bytea",
        supplied: &[17, 17],
        resolved: &[17, 17],
        text_inputs: &[&[92, 120, 48, 49], &[92, 120, 48, 50]],
        binary_inputs: &[&[1], &[2]],
        column: (23, 4, -1),
        text_output: &[50],
        binary_output: &[0, 0, 0, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bytea_text_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT b || $1 AS value FROM operator_inputs",
        supplied: &[25],
        resolved: &[25],
        text_inputs: &[&[122]],
        binary_inputs: &[&[122]],
        column: (25, -1, -1),
        text_output: &[92, 120, 48, 49, 122],
        binary_output: &[92, 120, 48, 49, 122],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_remainder_column_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT id % $1 AS value FROM operator_inputs",
        supplied: &[23],
        resolved: &[23],
        text_inputs: &[&[52]],
        binary_inputs: &[&[0, 0, 0, 4]],
        column: (23, 4, -1),
        text_output: &[49],
        binary_output: &[0, 0, 0, 1],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_remainder_cast_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1::int % 4 AS value",
        supplied: &[0],
        resolved: &[23],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 0, 0, 7]],
        column: (23, 4, -1),
        text_output: &[51],
        binary_output: &[0, 0, 0, 3],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_remainder_unknown_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 % 4 AS value",
        supplied: &[0],
        resolved: &[23],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 0, 0, 7]],
        column: (23, 4, -1),
        text_output: &[51],
        binary_output: &[0, 0, 0, 3],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_remainder_bare_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 % $2 AS value",
        supplied: &[23, 23],
        resolved: &[23, 23],
        text_inputs: &[&[53], &[52]],
        binary_inputs: &[&[0, 0, 0, 5], &[0, 0, 0, 4]],
        column: (23, 4, -1),
        text_output: &[49],
        binary_output: &[0, 0, 0, 1],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_add_bigint_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 + 1 AS value",
        supplied: &[20],
        resolved: &[20],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 0, 0, 0, 0, 0, 0, 7]],
        column: (20, 8, -1),
        text_output: &[56],
        binary_output: &[0, 0, 0, 0, 0, 0, 0, 8],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_add_smallint_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 + 1::smallint AS value",
        supplied: &[21],
        resolved: &[21],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 7]],
        column: (21, 2, -1),
        text_output: &[56],
        binary_output: &[0, 8],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_divide_int_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 / 4 AS value",
        supplied: &[23],
        resolved: &[23],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 0, 0, 7]],
        column: (23, 4, -1),
        text_output: &[49],
        binary_output: &[0, 0, 0, 1],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_subtract_int_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 - 4 AS value",
        supplied: &[23],
        resolved: &[23],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 0, 0, 7]],
        column: (23, 4, -1),
        text_output: &[51],
        binary_output: &[0, 0, 0, 3],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_multiply_int_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 * 4 AS value",
        supplied: &[23],
        resolved: &[23],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 0, 0, 7]],
        column: (23, 4, -1),
        text_output: &[50, 56],
        binary_output: &[0, 0, 0, 28],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_remainder_bigint_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 % 4 AS value",
        supplied: &[20],
        resolved: &[20],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 0, 0, 0, 0, 0, 0, 7]],
        column: (20, 8, -1),
        text_output: &[51],
        binary_output: &[0, 0, 0, 0, 0, 0, 0, 3],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_remainder_smallint_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 % 4::smallint AS value",
        supplied: &[21],
        resolved: &[21],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 7]],
        column: (21, 2, -1),
        text_output: &[51],
        binary_output: &[0, 3],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bytea_cte_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "WITH t AS (SELECT $1 || $2 AS value) SELECT value FROM t",
        supplied: &[17, 17],
        resolved: &[17, 17],
        text_inputs: &[&[92, 120, 48, 49], &[92, 120, 48, 50]],
        binary_inputs: &[&[1], &[2]],
        column: (17, -1, -1),
        text_output: &[92, 120, 48, 49, 48, 50],
        binary_output: &[1, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bytea_subquery_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT value FROM (SELECT $1 || '\\x02'::bytea AS value) AS t",
        supplied: &[17],
        resolved: &[17],
        text_inputs: &[&[92, 120, 48, 49]],
        binary_inputs: &[&[1]],
        column: (17, -1, -1),
        text_output: &[92, 120, 48, 49, 48, 50],
        binary_output: &[1, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_bigint_subquery_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT value FROM (SELECT $1 + 1 AS value) AS t",
        supplied: &[20],
        resolved: &[20],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 0, 0, 0, 0, 0, 0, 7]],
        column: (20, 8, -1),
        text_output: &[56],
        binary_output: &[0, 0, 0, 0, 0, 0, 0, 8],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_mixed_remainder_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1::smallint % $2::bigint AS value",
        supplied: &[21, 20],
        resolved: &[21, 20],
        text_inputs: &[&[55], &[52]],
        binary_inputs: &[&[0, 7], &[0, 0, 0, 0, 0, 0, 0, 4]],
        column: (20, 8, -1),
        text_output: &[51],
        binary_output: &[0, 0, 0, 0, 0, 0, 0, 3],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_shift_smallint_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 << $2 AS value",
        supplied: &[21, 23],
        resolved: &[21, 23],
        text_inputs: &[&[55], &[49]],
        binary_inputs: &[&[0, 7], &[0, 0, 0, 1]],
        column: (21, 2, -1),
        text_output: &[49, 52],
        binary_output: &[0, 14],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_resolve_add_float_given_parameter_operators() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 + 1::float8 AS value",
        supplied: &[20],
        resolved: &[20],
        text_inputs: &[&[55]],
        binary_inputs: &[&[0, 0, 0, 0, 0, 0, 0, 7]],
        column: (701, 8, -1),
        text_output: &[56],
        binary_output: &[64, 32, 0, 0, 0, 0, 0, 0],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_reject_bytea_boolean_given_parameter_operator_validation() {
    // Arrange
    let sql = "SELECT '\\x01'::bytea || $1";
    // Act
    let frames = crate::support_parameter_operators::run_rejected(sql, &[16], &["true"]);
    // Assert
    crate::support_parameter_operators::assert_rejected(&frames);
}

#[test]
fn should_reject_unresolved_remainder_given_unknown_parameter_types() {
    // Arrange
    let sql = "SELECT $1 % $2";
    // Act
    let frames = crate::support_parameter_operators::run_rejected(sql, &[0, 0], &["7", "4"]);
    // Assert
    crate::support_parameter_operators::assert_rejected(&frames);
}

#[test]
fn should_reject_integer_text_remainder_given_declared_parameter_types() {
    // Arrange
    let sql = "SELECT $1 % $2";
    // Act
    let frames = crate::support_parameter_operators::run_rejected(sql, &[23, 25], &["7", "4"]);
    // Assert
    crate::support_parameter_operators::assert_rejected(&frames);
}

#[test]
fn should_resolve_bytea_coalesce_given_parameter_operator_inference() {
    // Arrange
    let case = Case {
        sql: "SELECT COALESCE($1 || $2, '\\x00'::bytea) AS value",
        supplied: &[17, 17],
        resolved: &[17, 17],
        text_inputs: &[b"\\x01", b"\\x02"],
        binary_inputs: &[&[1], &[2]],
        column: (17, -1, -1),
        text_output: b"\\x0102",
        binary_output: &[1, 2],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}

#[test]
fn should_reject_bytea_concat_numeric_comparison_given_declared_parameters() {
    // Arrange
    let sql = "SELECT $1 || $2 = 3";
    // Act
    let frames =
        crate::support_parameter_operators::run_rejected(sql, &[17, 17], &["\\x01", "\\x02"]);
    // Assert
    crate::support_parameter_operators::assert_rejected(&frames);
}

#[test]
fn should_reject_text_concat_numeric_comparison_given_declared_parameters() {
    // Arrange
    let sql = "SELECT $1 || $2 = 3";
    // Act
    let frames = crate::support_parameter_operators::run_rejected(sql, &[25, 25], &["1", "2"]);
    // Assert
    crate::support_parameter_operators::assert_rejected(&frames);
}

#[test]
fn should_reject_bytea_concat_uuid_comparison_given_declared_parameters() {
    // Arrange
    let sql = "SELECT $1 || $2 = '00000000-0000-0000-0000-000000000000'::uuid";
    // Act
    let frames =
        crate::support_parameter_operators::run_rejected(sql, &[17, 17], &["\\x01", "\\x02"]);
    // Assert
    crate::support_parameter_operators::assert_rejected(&frames);
}

#[test]
fn should_coerce_unknown_literal_given_bytea_concat_comparison() {
    // Arrange
    let case = Case {
        sql: "SELECT $1 || $2 = '\\x0102' AS value",
        supplied: &[17, 17],
        resolved: &[17, 17],
        text_inputs: &[b"\\x01", b"\\x02"],
        binary_inputs: &[&[1], &[2]],
        column: (16, 1, -1),
        text_output: b"t",
        binary_output: &[1],
    };
    // Act
    let batches = run(&case);
    // Assert
    assert_case(&case, &batches);
}
