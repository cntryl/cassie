//! Native option-table descriptors and array emission order.
use crate::support_bind_planning as planning;
use crate::support_options_wire;

#[test]
fn should_expand_options_0_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(NULL::text[])";
    // Act
    let batches = planning::pruned("options_0_bare", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_0_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(NULL::text[])";
    // Act
    let batches = planning::pruned("options_0_qualified", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_0_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(NULL::text[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_0_cte", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_0_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(NULL::text[]) AS q";
    // Act
    let batches = planning::pruned("options_0_alias", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_1_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(NULL)";
    // Act
    let batches = planning::pruned("options_1_bare", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_1_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(NULL)";
    // Act
    let batches = planning::pruned("options_1_qualified", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_1_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(NULL)) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_1_cte", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_1_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(NULL) AS q";
    // Act
    let batches = planning::pruned("options_1_alias", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_2_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY[]::text[])";
    // Act
    let batches = planning::pruned("options_2_bare", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_2_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY[]::text[])";
    // Act
    let batches = planning::pruned("options_2_qualified", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_2_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY[]::text[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_2_cte", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_2_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY[]::text[]) AS q";
    // Act
    let batches = planning::pruned("options_2_alias", sql);
    // Assert
    support_options_wire::assert_options(&batches, [&[], &[]]);
}

#[test]
fn should_expand_options_3_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['a=b']::text[])";
    // Act
    let batches = planning::pruned("options_3_bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_expand_options_3_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY['a=b']::text[])";
    // Act
    let batches = planning::pruned("options_3_qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_expand_options_3_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY['a=b']::text[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_3_cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_expand_options_3_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql =
        "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY['a=b']::text[]) AS q";
    // Act
    let batches = planning::pruned("options_3_alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
            &[&[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 1, 98]],
        ],
    );
}

#[test]
fn should_expand_options_4_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['a=','=v','=']::text[])";
    // Act
    let batches = planning::pruned("options_4_bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
            ],
        ],
    );
}

#[test]
fn should_expand_options_4_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY['a=','=v','=']::text[])";
    // Act
    let batches = planning::pruned("options_4_qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
            ],
        ],
    );
}

#[test]
fn should_expand_options_4_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY['a=','=v','=']::text[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_4_cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
            ],
        ],
    );
}

#[test]
fn should_expand_options_4_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY['a=','=v','=']::text[]) AS q";
    // Act
    let batches = planning::pruned("options_4_alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
            ],
            &[
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 0],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 118],
                &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0],
            ],
        ],
    );
}

#[test]
fn should_expand_options_5_bare_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_options_to_table(ARRAY['without','a=b=c']::text[])";
    // Act
    let batches = planning::pruned("options_5_bare", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
            ],
        ],
    );
}

#[test]
fn should_expand_options_5_qualified_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_catalog.pg_options_to_table(ARRAY['without','a=b=c']::text[])";
    // Act
    let batches = planning::pruned("options_5_qualified", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
            ],
        ],
    );
}

#[test]
fn should_expand_options_5_cte_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_options_to_table(ARRAY['without','a=b=c']::text[])) SELECT * FROM q";
    // Act
    let batches = planning::pruned("options_5_cte", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
            ],
        ],
    );
}

#[test]
fn should_expand_options_5_alias_given_text_arrays_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT q.option_name,q.option_value FROM pg_options_to_table(ARRAY['without','a=b=c']::text[]) AS q";
    // Act
    let batches = planning::pruned("options_5_alias", sql);
    // Assert
    support_options_wire::assert_options(
        &batches,
        [
            &[
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
            ],
            &[
                &[
                    0, 2, 0, 0, 0, 7, 119, 105, 116, 104, 111, 117, 116, 255, 255, 255, 255,
                ],
                &[0, 2, 0, 0, 0, 1, 97, 0, 0, 0, 3, 98, 61, 99],
            ],
        ],
    );
}
