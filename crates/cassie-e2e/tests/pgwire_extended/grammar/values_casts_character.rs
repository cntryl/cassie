//! Native character common types and source-aware coercion.
use crate::support_bind_planning as planning;
use crate::support_values_casts;

#[test]
fn should_preserve_unknown_null_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),(NULL) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-unknown_null", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_typed_null_same_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),(NULL::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-typed_null_same", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, 7),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_typed_null_other_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),(NULL::CHAR(5)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-typed_null_other", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 3, 97, 32, 32], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_char_text_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),('long🙂 '::TEXT) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-char_text", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_text_char_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('long🙂 '::TEXT),('a'::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-text_char", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 25, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_text_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('ab'::VARCHAR(4)),('long🙂 '::TEXT) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-varchar_text", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 2, 97, 98],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 2, 97, 98],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_text_varchar_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('long🙂 '::TEXT),('ab'::VARCHAR(4)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-text_varchar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 25, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 2, 97, 98],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 2, 97, 98],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_varchar_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),('long🙂 '::VARCHAR(9)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-char_varchar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_varchar_char_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('long🙂 '::VARCHAR(9)),('a'::CHAR(3)) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-varchar_char", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_unknown_spaces_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('a'::CHAR(3)),('long🙂 ') ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-unknown_spaces", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 3, 97, 32, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_unknown_unicode_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('a🙂'::CHAR(3)),('long🙂 ') ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-unknown_unicode", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 6, 97, 240, 159, 153, 130, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
            &[
                &[0, 1, 0, 0, 0, 6, 97, 240, 159, 153, 130, 32],
                &[0, 1, 0, 0, 0, 9, 108, 111, 110, 103, 240, 159, 153, 130, 32],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_to_text_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "SELECT v::TEXT AS output FROM (VALUES (1,'🙂'::CHAR(3)),(2,'long🙂 ')) AS q(k,v) ORDER BY k";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-char_to_text", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 4, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
        ],
    );
}

#[test]
fn should_preserve_char_to_varchar_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "SELECT v::VARCHAR AS output FROM (VALUES (1,'🙂'::CHAR(3)),(2,'long🙂 ')) AS q(k,v) ORDER BY k";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-char_to_varchar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 4, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 240, 159, 153, 130],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
        ],
    );
}

#[test]
fn should_preserve_mixed_to_text_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "SELECT v::TEXT AS output FROM (VALUES ('a'::CHAR(3)),('long🙂 '::VARCHAR(9))) AS q(v) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-mixed_to_text", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 25, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
        ],
    );
}

#[test]
fn should_preserve_mixed_to_varchar_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "SELECT v::VARCHAR AS output FROM (VALUES ('a'::CHAR(3)),('long🙂 '::VARCHAR(9))) AS q(v) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-mixed_to_varchar", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 1043, -1, -1),
        [
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
            &[
                &[0, 1, 0, 0, 0, 1, 97],
                &[0, 1, 0, 0, 0, 8, 108, 111, 110, 103, 240, 159, 153, 130],
            ],
        ],
    );
}

#[test]
fn should_preserve_default_width_given_character_values_when_native_typmods_are_selected() {
    // Arrange
    let sql = "VALUES ('a'::CHAR),('b'::CHAR) ORDER BY 1 NULLS LAST";
    // Act
    let batches = planning::pruned("valuescasts-charprobe-default_width", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("column1", 1042, -1, 5),
        [
            &[&[0, 1, 0, 0, 0, 1, 97], &[0, 1, 0, 0, 0, 1, 98]],
            &[&[0, 1, 0, 0, 0, 1, 97], &[0, 1, 0, 0, 0, 1, 98]],
        ],
    );
}
