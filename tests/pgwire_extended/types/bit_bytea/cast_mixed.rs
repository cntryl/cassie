//! Common integer source widths are selected before BYTEA conversion.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_promote_int2_int4_given_mixed_integer_values_when_bytea_conversion_uses_the_common_width()
{
    // Arrange
    let sql = "SELECT v::BYTEA AS output FROM (VALUES (1::INT2),(-1::INT4)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("byteacasts-mixed-int2_int4", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 10, 92, 120, 102, 102, 102, 102, 102, 102, 102, 102,
                ],
                &[0, 1, 0, 0, 0, 10, 92, 120, 48, 48, 48, 48, 48, 48, 48, 49],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 255, 255, 255, 255],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
            ],
        ],
    );
}

#[test]
fn should_promote_int2_int8_given_mixed_integer_values_when_bytea_conversion_uses_the_common_width()
{
    // Arrange
    let sql = "SELECT v::BYTEA AS output FROM (VALUES (1::INT2),(-1::INT8)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("byteacasts-mixed-int2_int8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 18, 92, 120, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
                    102, 102, 102, 102, 102, 102,
                ],
                &[
                    0, 1, 0, 0, 0, 18, 92, 120, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
                    48, 48, 49,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 255],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
            ],
        ],
    );
}

#[test]
fn should_promote_int4_int8_given_mixed_integer_values_when_bytea_conversion_uses_the_common_width()
{
    // Arrange
    let sql = "SELECT v::BYTEA AS output FROM (VALUES (1::INT4),(-1::INT8)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("byteacasts-mixed-int4_int8", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 18, 92, 120, 102, 102, 102, 102, 102, 102, 102, 102, 102, 102,
                    102, 102, 102, 102, 102, 102,
                ],
                &[
                    0, 1, 0, 0, 0, 18, 92, 120, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
                    48, 48, 49,
                ],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 255, 255, 255, 255, 255, 255, 255, 255],
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
            ],
        ],
    );
}

#[test]
fn should_promote_typed_null_given_mixed_integer_values_when_bytea_conversion_uses_the_common_width(
) {
    // Arrange
    let sql = "SELECT v::BYTEA AS output FROM (VALUES (1::INT2),(NULL::INT8)) AS q(v) ORDER BY v NULLS LAST";
    // Act
    let batches = planning::pruned("byteacasts-mixed-typed_null", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 17, -1, -1),
        [
            &[
                &[
                    0, 1, 0, 0, 0, 18, 92, 120, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48, 48,
                    48, 48, 49,
                ],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}
