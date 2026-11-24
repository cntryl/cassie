//! Native option-table descriptors and array emission order.
use crate::support_bind_planning as planning;
use crate::support_options_wire;

#[test]
fn should_expand_options_6_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY[' a = b ','x= y']::text[])";
    // Act
    let batches = planning::pruned("options_6_bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 3, 32, 97, 32, 0, 0, 0, 3, 32, 98, 32],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 2, 32, 121],
            ],
            &[
                &[0, 2, 0, 0, 0, 3, 32, 97, 32, 0, 0, 0, 3, 32, 98, 32],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 2, 32, 121],
            ],
        ],
    );
}

#[test]
fn should_expand_options_6_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY[' a = b ','x= y']::text[])";
    // Act
    let batches = planning::pruned("options_6_qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 3, 32, 97, 32, 0, 0, 0, 3, 32, 98, 32],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 2, 32, 121],
            ],
            &[
                &[0, 2, 0, 0, 0, 3, 32, 97, 32, 0, 0, 0, 3, 32, 98, 32],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 2, 32, 121],
            ],
        ],
    );
}

#[test]
fn should_expand_options_6_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY[' a = b ','x= y']::text[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_6_cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 3, 32, 97, 32, 0, 0, 0, 3, 32, 98, 32],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 2, 32, 121],
            ],
            &[
                &[0, 2, 0, 0, 0, 3, 32, 97, 32, 0, 0, 0, 3, 32, 98, 32],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 2, 32, 121],
            ],
        ],
    );
}

#[test]
fn should_expand_options_6_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY[' a = b ','x= y']::text[]) AS q";
    // Act
    let batches = planning::pruned("options_6_alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 3, 32, 97, 32, 0, 0, 0, 3, 32, 98, 32],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 2, 32, 121],
            ],
            &[
                &[0, 2, 0, 0, 0, 3, 32, 97, 32, 0, 0, 0, 3, 32, 98, 32],
                &[0, 2, 0, 0, 0, 1, 120, 0, 0, 0, 2, 32, 121],
            ],
        ],
    );
}

#[test]
fn should_expand_options_7_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['','NULL=null']::text[])";
    // Act
    let batches = planning::pruned("options_7_bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 0, 255, 255, 255, 255],
                &[
                    0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 0, 0, 0, 4, 110, 117, 108, 108,
                ],
            ],
            &[
                &[0, 2, 0, 0, 0, 0, 255, 255, 255, 255],
                &[
                    0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 0, 0, 0, 4, 110, 117, 108, 108,
                ],
            ],
        ],
    );
}

#[test]
fn should_expand_options_7_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY['','NULL=null']::text[])";
    // Act
    let batches = planning::pruned("options_7_qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 0, 255, 255, 255, 255],
                &[
                    0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 0, 0, 0, 4, 110, 117, 108, 108,
                ],
            ],
            &[
                &[0, 2, 0, 0, 0, 0, 255, 255, 255, 255],
                &[
                    0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 0, 0, 0, 4, 110, 117, 108, 108,
                ],
            ],
        ],
    );
}

#[test]
fn should_expand_options_7_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY['','NULL=null']::text[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_7_cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 0, 255, 255, 255, 255],
                &[
                    0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 0, 0, 0, 4, 110, 117, 108, 108,
                ],
            ],
            &[
                &[0, 2, 0, 0, 0, 0, 255, 255, 255, 255],
                &[
                    0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 0, 0, 0, 4, 110, 117, 108, 108,
                ],
            ],
        ],
    );
}

#[test]
fn should_expand_options_7_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY['','NULL=null']::text[]) AS q";
    // Act
    let batches = planning::pruned("options_7_alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 0, 255, 255, 255, 255],
                &[
                    0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 0, 0, 0, 4, 110, 117, 108, 108,
                ],
            ],
            &[
                &[0, 2, 0, 0, 0, 0, 255, 255, 255, 255],
                &[
                    0, 2, 0, 0, 0, 4, 78, 85, 76, 76, 0, 0, 0, 4, 110, 117, 108, 108,
                ],
            ],
        ],
    );
}

#[test]
fn should_expand_options_8_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['a=1','a=2','a=1']::text[])";
    // Act
    let batches = planning::pruned("options_8_bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
            ],
        ],
    );
}

#[test]
fn should_expand_options_8_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY['a=1','a=2','a=1']::text[])";
    // Act
    let batches = planning::pruned("options_8_qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
            ],
        ],
    );
}

#[test]
fn should_expand_options_8_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY['a=1','a=2','a=1']::text[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_8_cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
            ],
        ],
    );
}

#[test]
fn should_expand_options_8_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY['a=1','a=2','a=1']::text[]) AS q";
    // Act
    let batches = planning::pruned("options_8_alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 50],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 49],
            ],
        ],
    );
}

#[test]
fn should_expand_options_9_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['Δ=世界','emoji=😀']::text[])";
    // Act
    let batches = planning::pruned("options_9_bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 101, 109, 111, 106, 105, 0, 0, 0, 4, 240, 159, 152, 128,
                ],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 101, 109, 111, 106, 105, 0, 0, 0, 4, 240, 159, 152, 128,
                ],
            ],
        ],
    );
}

#[test]
fn should_expand_options_9_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY['Δ=世界','emoji=😀']::text[])";
    // Act
    let batches = planning::pruned("options_9_qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 101, 109, 111, 106, 105, 0, 0, 0, 4, 240, 159, 152, 128,
                ],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 101, 109, 111, 106, 105, 0, 0, 0, 4, 240, 159, 152, 128,
                ],
            ],
        ],
    );
}

#[test]
fn should_expand_options_9_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY['Δ=世界','emoji=😀']::text[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_9_cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 101, 109, 111, 106, 105, 0, 0, 0, 4, 240, 159, 152, 128,
                ],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 101, 109, 111, 106, 105, 0, 0, 0, 4, 240, 159, 152, 128,
                ],
            ],
        ],
    );
}

#[test]
fn should_expand_options_9_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY['Δ=世界','emoji=😀']::text[]) AS q";
    // Act
    let batches = planning::pruned("options_9_alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 101, 109, 111, 106, 105, 0, 0, 0, 4, 240, 159, 152, 128,
                ],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 2, 206, 148, 0, 0, 0, 6, 228, 184, 150, 231, 149, 140,
                ],
                &[
                    0, 2, 0, 0, 0, 5, 101, 109, 111, 106, 105, 0, 0, 0, 4, 240, 159, 152, 128,
                ],
            ],
        ],
    );
}

#[test]
fn should_expand_options_10_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table('{a=b,c=d}')";
    // Act
    let batches = planning::pruned("options_10_bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 1, 99, 0, 0, 0, 1, 100],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 1, 99, 0, 0, 0, 1, 100],
            ],
        ],
    );
}

#[test]
fn should_expand_options_10_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table('{a=b,c=d}')";
    // Act
    let batches = planning::pruned("options_10_qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 1, 99, 0, 0, 0, 1, 100],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 1, 99, 0, 0, 0, 1, 100],
            ],
        ],
    );
}

#[test]
fn should_expand_options_10_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table('{a=b,c=d}')) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_10_cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 1, 99, 0, 0, 0, 1, 100],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 1, 99, 0, 0, 0, 1, 100],
            ],
        ],
    );
}

#[test]
fn should_expand_options_10_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table('{a=b,c=d}') AS q";
    // Act
    let batches = planning::pruned("options_10_alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 1, 99, 0, 0, 0, 1, 100],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98],
                &[0, 2, 0, 0, 0, 1, 99, 0, 0, 0, 1, 100],
            ],
        ],
    );
}

#[test]
fn should_expand_options_11_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table('{\"comma=a,b\",\"quote=c\"}')";
    // Act
    let batches = planning::pruned("options_11_bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 5, 113, 117, 111, 116, 101, 0, 0, 0, 1, 99],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 5, 113, 117, 111, 116, 101, 0, 0, 0, 1, 99],
            ],
        ],
    );
}

#[test]
fn should_expand_options_11_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table('{\"comma=a,b\",\"quote=c\"}')";
    // Act
    let batches = planning::pruned("options_11_qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 5, 113, 117, 111, 116, 101, 0, 0, 0, 1, 99],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 5, 113, 117, 111, 116, 101, 0, 0, 0, 1, 99],
            ],
        ],
    );
}

#[test]
fn should_expand_options_11_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table('{\"comma=a,b\",\"quote=c\"}')) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_11_cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 5, 113, 117, 111, 116, 101, 0, 0, 0, 1, 99],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 5, 113, 117, 111, 116, 101, 0, 0, 0, 1, 99],
            ],
        ],
    );
}

#[test]
fn should_expand_options_11_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table('{\"comma=a,b\",\"quote=c\"}') AS q";
    // Act
    let batches = planning::pruned("options_11_alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 5, 113, 117, 111, 116, 101, 0, 0, 0, 1, 99],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 5, 99, 111, 109, 109, 97, 0, 0, 0, 3, 97, 44, 98,
                ],
                &[0, 2, 0, 0, 0, 5, 113, 117, 111, 116, 101, 0, 0, 0, 1, 99],
            ],
        ],
    );
}
