//! Literal escaping survives selected TEXT[] constructor lowering.
use crate::support_bind_planning as planning;
use crate::support_options_wire;

#[test]
fn should_preserve_escaping_0_bare_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['a=\"quoted\"',E'path=c:\\\\tmp','comma=a,b','NULL']::TEXT[])";
    // Act
    let batches = planning::pruned("options-escaping-0-bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 8, 34, 113, 117, 111, 116, 101, 100, 34,
                ],
                &[
                    0, 2, 0, 0, 0, 4, 112, 97, 116, 104, 0, 0, 0, 6, 99, 58, 92, 116, 109, 112,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 8, 34, 113, 117, 111, 116, 101, 100, 34,
                ],
                &[
                    0, 2, 0, 0, 0, 4, 112, 97, 116, 104, 0, 0, 0, 6, 99, 58, 92, 116, 109, 112,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_0_qualified_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY['a=\"quoted\"',E'path=c:\\\\tmp','comma=a,b','NULL']::TEXT[])";
    // Act
    let batches = planning::pruned("options-escaping-0-qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 8, 34, 113, 117, 111, 116, 101, 100, 34,
                ],
                &[
                    0, 2, 0, 0, 0, 4, 112, 97, 116, 104, 0, 0, 0, 6, 99, 58, 92, 116, 109, 112,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 8, 34, 113, 117, 111, 116, 101, 100, 34,
                ],
                &[
                    0, 2, 0, 0, 0, 4, 112, 97, 116, 104, 0, 0, 0, 6, 99, 58, 92, 116, 109, 112,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_0_cte_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY['a=\"quoted\"',E'path=c:\\\\tmp','comma=a,b','NULL']::TEXT[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options-escaping-0-cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 8, 34, 113, 117, 111, 116, 101, 100, 34,
                ],
                &[
                    0, 2, 0, 0, 0, 4, 112, 97, 116, 104, 0, 0, 0, 6, 99, 58, 92, 116, 109, 112,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 8, 34, 113, 117, 111, 116, 101, 100, 34,
                ],
                &[
                    0, 2, 0, 0, 0, 4, 112, 97, 116, 104, 0, 0, 0, 6, 99, 58, 92, 116, 109, 112,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_0_alias_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY['a=\"quoted\"',E'path=c:\\\\tmp','comma=a,b','NULL']::TEXT[]) AS q";
    // Act
    let batches = planning::pruned("options-escaping-0-alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 8, 34, 113, 117, 111, 116, 101, 100, 34,
                ],
                &[
                    0, 2, 0, 0, 0, 4, 112, 97, 116, 104, 0, 0, 0, 6, 99, 58, 92, 116, 109, 112,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 8, 34, 113, 117, 111, 116, 101, 100, 34,
                ],
                &[
                    0, 2, 0, 0, 0, 4, 112, 97, 116, 104, 0, 0, 0, 6, 99, 58, 92, 116, 109, 112,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_1_bare_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['a=''quote''','x=☃']::TEXT[])";
    // Act
    let batches = planning::pruned("options-escaping-1-bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 7, 39, 113, 117, 111, 116, 101, 39,
                ],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 3, 226, 152, 131],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 7, 39, 113, 117, 111, 116, 101, 39,
                ],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 3, 226, 152, 131],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_1_qualified_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY['a=''quote''','x=☃']::TEXT[])";
    // Act
    let batches = planning::pruned("options-escaping-1-qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 7, 39, 113, 117, 111, 116, 101, 39,
                ],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 3, 226, 152, 131],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 7, 39, 113, 117, 111, 116, 101, 39,
                ],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 3, 226, 152, 131],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_1_cte_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY['a=''quote''','x=☃']::TEXT[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options-escaping-1-cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 7, 39, 113, 117, 111, 116, 101, 39,
                ],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 3, 226, 152, 131],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 7, 39, 113, 117, 111, 116, 101, 39,
                ],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 3, 226, 152, 131],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_1_alias_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY['a=''quote''','x=☃']::TEXT[]) AS q";
    // Act
    let batches = planning::pruned("options-escaping-1-alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 7, 39, 113, 117, 111, 116, 101, 39,
                ],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 3, 226, 152, 131],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 7, 39, 113, 117, 111, 116, 101, 39,
                ],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 3, 226, 152, 131],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_2_bare_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['bracket=]','brace=\x7b','equals===','backslash=\\']::TEXT[])";
    // Act
    let batches = planning::pruned("options-escaping-2-bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 7, 98, 114, 97, 99, 107, 101, 116, 0, 0, 0, 1, 93,
                ],
                &[0, 2, 0, 0, 0, 5, 98, 114, 97, 99, 101, 0, 0, 0, 1, 123],
                &[
                    0, 2, 0, 0, 0, 6, 101, 113, 117, 97, 108, 115, 0, 0, 0, 2, 61, 61,
                ],
                &[
                    0, 2, 0, 0, 0, 9, 98, 97, 99, 107, 115, 108, 97, 115, 104, 0, 0, 0, 1, 92,
                ],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 7, 98, 114, 97, 99, 107, 101, 116, 0, 0, 0, 1, 93,
                ],
                &[0, 2, 0, 0, 0, 5, 98, 114, 97, 99, 101, 0, 0, 0, 1, 123],
                &[
                    0, 2, 0, 0, 0, 6, 101, 113, 117, 97, 108, 115, 0, 0, 0, 2, 61, 61,
                ],
                &[
                    0, 2, 0, 0, 0, 9, 98, 97, 99, 107, 115, 108, 97, 115, 104, 0, 0, 0, 1, 92,
                ],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_2_qualified_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY['bracket=]','brace=\x7b','equals===','backslash=\\']::TEXT[])";
    // Act
    let batches = planning::pruned("options-escaping-2-qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 7, 98, 114, 97, 99, 107, 101, 116, 0, 0, 0, 1, 93,
                ],
                &[0, 2, 0, 0, 0, 5, 98, 114, 97, 99, 101, 0, 0, 0, 1, 123],
                &[
                    0, 2, 0, 0, 0, 6, 101, 113, 117, 97, 108, 115, 0, 0, 0, 2, 61, 61,
                ],
                &[
                    0, 2, 0, 0, 0, 9, 98, 97, 99, 107, 115, 108, 97, 115, 104, 0, 0, 0, 1, 92,
                ],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 7, 98, 114, 97, 99, 107, 101, 116, 0, 0, 0, 1, 93,
                ],
                &[0, 2, 0, 0, 0, 5, 98, 114, 97, 99, 101, 0, 0, 0, 1, 123],
                &[
                    0, 2, 0, 0, 0, 6, 101, 113, 117, 97, 108, 115, 0, 0, 0, 2, 61, 61,
                ],
                &[
                    0, 2, 0, 0, 0, 9, 98, 97, 99, 107, 115, 108, 97, 115, 104, 0, 0, 0, 1, 92,
                ],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_2_cte_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY['bracket=]','brace=\x7b','equals===','backslash=\\']::TEXT[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options-escaping-2-cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 7, 98, 114, 97, 99, 107, 101, 116, 0, 0, 0, 1, 93,
                ],
                &[0, 2, 0, 0, 0, 5, 98, 114, 97, 99, 101, 0, 0, 0, 1, 123],
                &[
                    0, 2, 0, 0, 0, 6, 101, 113, 117, 97, 108, 115, 0, 0, 0, 2, 61, 61,
                ],
                &[
                    0, 2, 0, 0, 0, 9, 98, 97, 99, 107, 115, 108, 97, 115, 104, 0, 0, 0, 1, 92,
                ],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 7, 98, 114, 97, 99, 107, 101, 116, 0, 0, 0, 1, 93,
                ],
                &[0, 2, 0, 0, 0, 5, 98, 114, 97, 99, 101, 0, 0, 0, 1, 123],
                &[
                    0, 2, 0, 0, 0, 6, 101, 113, 117, 97, 108, 115, 0, 0, 0, 2, 61, 61,
                ],
                &[
                    0, 2, 0, 0, 0, 9, 98, 97, 99, 107, 115, 108, 97, 115, 104, 0, 0, 0, 1, 92,
                ],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_2_alias_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY['bracket=]','brace=\x7b','equals===','backslash=\\']::TEXT[]) AS q";
    // Act
    let batches = planning::pruned("options-escaping-2-alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 7, 98, 114, 97, 99, 107, 101, 116, 0, 0, 0, 1, 93,
                ],
                &[0, 2, 0, 0, 0, 5, 98, 114, 97, 99, 101, 0, 0, 0, 1, 123],
                &[
                    0, 2, 0, 0, 0, 6, 101, 113, 117, 97, 108, 115, 0, 0, 0, 2, 61, 61,
                ],
                &[
                    0, 2, 0, 0, 0, 9, 98, 97, 99, 107, 115, 108, 97, 115, 104, 0, 0, 0, 1, 92,
                ],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 7, 98, 114, 97, 99, 107, 101, 116, 0, 0, 0, 1, 93,
                ],
                &[0, 2, 0, 0, 0, 5, 98, 114, 97, 99, 101, 0, 0, 0, 1, 123],
                &[
                    0, 2, 0, 0, 0, 6, 101, 113, 117, 97, 108, 115, 0, 0, 0, 2, 61, 61,
                ],
                &[
                    0, 2, 0, 0, 0, 9, 98, 97, 99, 107, 115, 108, 97, 115, 104, 0, 0, 0, 1, 92,
                ],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_3_bare_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(CAST(ARRAY['a=b','plain','NULL'] AS TEXT[]))";
    // Act
    let batches = planning::pruned("options-escaping-3-bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 5, 112, 108, 97, 105, 110, 255, 255, 255, 255],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 5, 112, 108, 97, 105, 110, 255, 255, 255, 255],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_3_qualified_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql =
        "SELECT * FROM pg_catalog.pg_options_to_table(CAST(ARRAY['a=b','plain','NULL'] AS TEXT[]))";
    // Act
    let batches = planning::pruned("options-escaping-3-qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 5, 112, 108, 97, 105, 110, 255, 255, 255, 255],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 5, 112, 108, 97, 105, 110, 255, 255, 255, 255],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_3_cte_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(CAST(ARRAY['a=b','plain','NULL'] AS TEXT[]))) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options-escaping-3-cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 5, 112, 108, 97, 105, 110, 255, 255, 255, 255],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 5, 112, 108, 97, 105, 110, 255, 255, 255, 255],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_escaping_3_alias_given_literal_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(CAST(ARRAY['a=b','plain','NULL'] AS TEXT[])) AS q";
    // Act
    let batches = planning::pruned("options-escaping-3-alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 5, 112, 108, 97, 105, 110, 255, 255, 255, 255],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 5, 112, 108, 97, 105, 110, 255, 255, 255, 255],
                &[0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 255, 255, 255, 255],
            ],
        ],
    );
}
