//! Native signed integer widths through BYTEA casts.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_encode_negative_32768_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (-32768)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_32768_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 56, 48, 48, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 128, 0]],
        ],
    );
}

#[test]
fn should_encode_negative_32768_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((-32768)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_32768_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 56, 48, 48, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 128, 0]],
        ],
    );
}

#[test]
fn should_encode_negative_32768_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((-32768)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_32768_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 56, 48, 48, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 128, 0]],
        ],
    );
}

#[test]
fn should_encode_negative_257_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (-257)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_257_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 101, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 254, 255]],
        ],
    );
}

#[test]
fn should_encode_negative_257_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((-257)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_257_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 101, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 254, 255]],
        ],
    );
}

#[test]
fn should_encode_negative_257_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((-257)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_257_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 101, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 254, 255]],
        ],
    );
}

#[test]
fn should_encode_negative_256_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (-256)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_256_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 48, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 0]],
        ],
    );
}

#[test]
fn should_encode_negative_256_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((-256)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_256_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 48, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 0]],
        ],
    );
}

#[test]
fn should_encode_negative_256_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((-256)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_256_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 48, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 0]],
        ],
    );
}

#[test]
fn should_encode_negative_129_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (-129)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_129_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 55, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 127]],
        ],
    );
}

#[test]
fn should_encode_negative_129_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((-129)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_129_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 55, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 127]],
        ],
    );
}

#[test]
fn should_encode_negative_129_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((-129)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_129_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 55, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 127]],
        ],
    );
}

#[test]
fn should_encode_negative_128_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (-128)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_128_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 56, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 128]],
        ],
    );
}

#[test]
fn should_encode_negative_128_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((-128)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_128_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 56, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 128]],
        ],
    );
}

#[test]
fn should_encode_negative_128_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((-128)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_128_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 56, 48,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 128]],
        ],
    );
}

#[test]
fn should_encode_negative_1_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (-1)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_1_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_encode_negative_1_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((-1)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_1_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_encode_negative_1_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((-1)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_1_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_encode_positive_0_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (0)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_0_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_encode_positive_0_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((0)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_0_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_encode_positive_0_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((0)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_0_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_encode_positive_1_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (1)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_1_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 48, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_encode_positive_1_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((1)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_1_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 48, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_encode_positive_1_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((1)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_1_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 48, 49]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_encode_positive_127_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (127)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_127_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 55, 102]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 127]],
        ],
    );
}

#[test]
fn should_encode_positive_127_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((127)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_127_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 55, 102]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 127]],
        ],
    );
}

#[test]
fn should_encode_positive_127_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((127)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_127_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 55, 102]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 127]],
        ],
    );
}

#[test]
fn should_encode_positive_128_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (128)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_128_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 56, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 128]],
        ],
    );
}

#[test]
fn should_encode_positive_128_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((128)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_128_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 56, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 128]],
        ],
    );
}

#[test]
fn should_encode_positive_128_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((128)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_128_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 56, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 128]],
        ],
    );
}

#[test]
fn should_encode_positive_255_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (255)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_255_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 102, 102]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 255]],
        ],
    );
}

#[test]
fn should_encode_positive_255_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((255)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_255_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 102, 102]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 255]],
        ],
    );
}

#[test]
fn should_encode_positive_255_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((255)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_255_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 102, 102]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 255]],
        ],
    );
}

#[test]
fn should_encode_positive_256_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (256)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_256_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 49, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_encode_positive_256_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((256)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_256_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 49, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_encode_positive_256_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((256)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_256_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 49, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 1, 0]],
        ],
    );
}

#[test]
fn should_encode_positive_32767_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (32767)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_32767_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 55, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 127, 255]],
        ],
    );
}

#[test]
fn should_encode_positive_32767_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((32767)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_32767_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 55, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 127, 255]],
        ],
    );
}

#[test]
fn should_encode_positive_32767_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((32767)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_32767_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 55, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 127, 255]],
        ],
    );
}

#[test]
fn should_encode_negative_2147483648_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (-2147483648)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_2147483648_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 56, 48, 48, 48, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 128, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_encode_negative_2147483648_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((-2147483648)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_2147483648_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 56, 48, 48, 48, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 128, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_encode_negative_2147483648_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((-2147483648)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_negative_2147483648_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 10, 92, 120, 56, 48, 48, 48, 48, 48, 48, 48]],
            &[&[0, 1, 0, 0, 0, 4, 128, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_encode_positive_2147483647_given_int4_source_when_scalar_casts_to_bytea() {
    // Arrange
    let sql = "SELECT (2147483647)::INT4::BYTEA AS output";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_2147483647_scalar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 55, 102, 102, 102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 127, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_encode_positive_2147483647_given_int4_source_when_values_casts_to_bytea() {
    // Arrange
    let sql = "VALUES ((2147483647)::INT4::BYTEA)";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_2147483647_values", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 55, 102, 102, 102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 127, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_encode_positive_2147483647_given_int4_source_when_cte_casts_to_bytea() {
    // Arrange
    let sql = "WITH q(v) AS (VALUES ((2147483647)::INT4)) SELECT v::BYTEA AS output FROM q";
    // Act
    let batches = planning::pruned("byteacasts-int4_positive_2147483647_cte", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[&[
                0, 1, 0, 0, 0, 10, 92, 120, 55, 102, 102, 102, 102, 102, 102, 102,
            ]],
            &[&[0, 1, 0, 0, 0, 4, 127, 255, 255, 255]],
        ],
    );
}
