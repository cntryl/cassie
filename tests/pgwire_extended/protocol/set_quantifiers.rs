//! Native wire identities and payloads for set quantifiers.
use crate::support_bind_planning as planning;
use crate::support_fetch;

#[test]
fn should_preserve_select_0_0_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) UNION SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-select_0_0", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_values_0_0_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (1),(1),(2),(NULL::INT) UNION VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-values_0_0", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_select_0_1_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) UNION ALL SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-select_0_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_values_0_1_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (1),(1),(2),(NULL::INT) UNION ALL VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-values_0_1", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
                &[0, 1, 255, 255, 255, 255],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_select_0_2_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) UNION DISTINCT SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-select_0_2", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_values_0_2_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (1),(1),(2),(NULL::INT) UNION DISTINCT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-values_0_2", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[
                &[0, 1, 0, 0, 0, 1, 49],
                &[0, 1, 0, 0, 0, 1, 50],
                &[0, 1, 0, 0, 0, 1, 51],
                &[0, 1, 255, 255, 255, 255],
            ],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 3],
                &[0, 1, 255, 255, 255, 255],
            ],
        ],
    );
}

#[test]
fn should_preserve_select_0_3_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) INTERSECT SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-select_0_3", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_values_0_3_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (1),(1),(2),(NULL::INT) INTERSECT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-values_0_3", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_select_0_4_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) INTERSECT ALL SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-select_0_4", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_values_0_4_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (1),(1),(2),(NULL::INT) INTERSECT ALL VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-values_0_4", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_select_0_5_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) INTERSECT DISTINCT SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-select_0_5", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_values_0_5_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (1),(1),(2),(NULL::INT) INTERSECT DISTINCT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-values_0_5", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 255, 255, 255, 255]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 1], &[0, 1, 255, 255, 255, 255]],
        ],
    );
}

#[test]
fn should_preserve_select_0_6_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) EXCEPT SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-select_0_6", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_preserve_values_0_6_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (1),(1),(2),(NULL::INT) EXCEPT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-values_0_6", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_preserve_select_0_7_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) EXCEPT ALL SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-select_0_7", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 0, 0, 0, 1, 50]],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
            ],
        ],
    );
}

#[test]
fn should_preserve_values_0_7_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (1),(1),(2),(NULL::INT) EXCEPT ALL VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-values_0_7", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 49], &[0, 1, 0, 0, 0, 1, 50]],
            &[
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
                &[0, 1, 0, 0, 0, 4, 0, 0, 0, 2],
            ],
        ],
    );
}

#[test]
fn should_preserve_select_0_8_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (SELECT ALL n FROM (VALUES (1),(1),(2),(NULL::INT)) AS l(n) EXCEPT DISTINCT SELECT ALL n FROM (VALUES (1),(3),(NULL::INT)) AS r(n) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-select_0_8", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}

#[test]
fn should_preserve_values_0_8_given_wire_formats_when_set_rows_are_described() {
    // Arrange
    let sql = "SELECT n::INT FROM (VALUES (1),(1),(2),(NULL::INT) EXCEPT DISTINCT VALUES (1),(3),(NULL::INT) ORDER BY 1 NULLS LAST) AS q(n) ORDER BY n NULLS LAST";
    // Act
    let batches = planning::pruned("setspell-wire-values_0_8", sql);
    // Assert
    support_fetch::assert_result(
        &batches,
        [
            &[&[0, 1, 0, 0, 0, 1, 50]],
            &[&[0, 1, 0, 0, 0, 4, 0, 0, 0, 2]],
        ],
    );
}
