//! Relation row types: every table owns a composite type named like the table.

use crate::support_composite_types::{first_column_text, only_text};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE t (id int, name text)",
            "INSERT INTO t VALUES (1, 'x')",
        ],
    )
}

fn text(fixture: &SqlFixture, sql: &str) -> Option<String> {
    only_text(&fixture.rows(sql))
}

#[test]
fn should_select_the_whole_row_by_table_name() {
    // Arrange
    let fixture = fixture("row_type_whole_row");

    // Act
    let row = text(&fixture, "SELECT t::text FROM t");

    // Assert
    assert_eq!(row.as_deref(), Some("(1,x)"));
}

#[test]
fn should_select_the_whole_row_by_alias() {
    // Arrange
    let fixture = fixture("row_type_alias_row");

    // Act
    let row = text(&fixture, "SELECT r::text FROM t AS r");

    // Assert
    assert_eq!(row.as_deref(), Some("(1,x)"));
}

#[test]
fn should_select_a_field_of_the_whole_row() {
    // Arrange
    let fixture = fixture("row_type_field_of_row");

    // Act
    let name = text(&fixture, "SELECT (t).name FROM t");

    // Assert
    assert_eq!(name.as_deref(), Some("x"));
}

#[test]
fn should_keep_qualified_star_expansion_unchanged() {
    // Arrange
    let fixture = fixture("row_type_qualified_star");

    // Act
    let rows = fixture.rows("SELECT t.* FROM t");

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 2);
}

#[test]
fn should_build_a_row_from_a_qualified_star() {
    // Arrange
    let fixture = fixture("row_type_row_of_star");

    // Act
    let row = text(&fixture, "SELECT ROW(t.*)::text FROM t");

    // Assert
    assert_eq!(row.as_deref(), Some("(1,x)"));
}

#[test]
fn should_cast_a_row_constructor_to_a_table_type() {
    // Arrange
    let fixture = fixture("row_type_cast");

    // Act
    let cast = text(&fixture, "SELECT ROW(2, 'y')::t::text");

    // Assert
    assert_eq!(cast.as_deref(), Some("(2,y)"));
}

#[test]
fn should_cast_a_text_literal_to_a_table_type() {
    // Arrange
    let fixture = fixture("row_type_text_cast");

    // Act
    let cast = text(&fixture, "SELECT '(3,z)'::t::text");

    // Assert
    assert_eq!(cast.as_deref(), Some("(3,z)"));
}

#[test]
fn should_expand_the_fields_of_a_cast_composite() {
    // Arrange
    let fixture = fixture("row_type_expand_cast");

    // Act
    let rows = fixture.rows("SELECT (ROW(2, 'y')::t).*");

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 2);
}

#[test]
fn should_expand_the_fields_of_a_composite_column() {
    // Arrange
    let fixture = sql_fixture(
        "row_type_expand_column",
        &[
            "CREATE TYPE address AS (street text, zip int)",
            "CREATE TABLE homes (id int, a address)",
            "INSERT INTO homes VALUES (1, ROW('1 Main', 11))",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT (a).* FROM homes");

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 2);
}

#[test]
fn should_reject_expanding_an_expression_of_unknown_type_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("row_type_expand_unknown");

    // Act
    let error = fixture.error("SELECT (id + 1).* FROM t");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_use_a_table_name_as_a_column_type() {
    // Arrange
    let fixture = fixture("row_type_column_type");
    fixture.execute("CREATE TABLE u (k int, x t)").unwrap();
    fixture
        .execute("INSERT INTO u VALUES (1, ROW(2, 'z'))")
        .unwrap();

    // Act
    let name = text(&fixture, "SELECT (x).name FROM u");

    // Assert
    assert_eq!(name.as_deref(), Some("z"));
}

#[test]
fn should_read_added_table_columns_as_null_inside_stored_row_values() {
    // Arrange
    let fixture = fixture("row_type_add_column");
    fixture.execute("CREATE TABLE u (k int, x t)").unwrap();
    fixture
        .execute("INSERT INTO u VALUES (1, ROW(2, 'z'))")
        .unwrap();
    fixture
        .execute("ALTER TABLE t ADD COLUMN extra int")
        .unwrap();

    // Act
    let stored = text(&fixture, "SELECT x::text FROM u");

    // Assert
    assert_eq!(stored.as_deref(), Some("(2,z,)"));
}

#[test]
fn should_follow_a_renamed_table_column_in_stored_row_values() {
    // Arrange
    let fixture = fixture("row_type_rename_column");
    fixture.execute("CREATE TABLE u (k int, x t)").unwrap();
    fixture
        .execute("INSERT INTO u VALUES (1, ROW(2, 'z'))")
        .unwrap();
    fixture
        .execute("ALTER TABLE t RENAME COLUMN name TO label")
        .unwrap();

    // Act
    let label = text(&fixture, "SELECT (x).label FROM u");

    // Assert
    assert_eq!(label.as_deref(), Some("z"));
}

#[test]
fn should_follow_a_renamed_table_when_resolving_the_row_type() {
    // Arrange
    let fixture = fixture("row_type_rename_table");
    fixture.execute("ALTER TABLE t RENAME TO items").unwrap();

    // Act
    let cast = text(&fixture, "SELECT '(5,q)'::items::text");

    // Assert
    assert_eq!(cast.as_deref(), Some("(5,q)"));
}

#[test]
fn should_refuse_dropping_a_table_whose_row_type_is_a_column_with_sqlstate_2bp01() {
    // Arrange
    let fixture = fixture("row_type_drop_in_use");
    fixture.execute("CREATE TABLE u (k int, x t)").unwrap();

    // Act
    let error = fixture.error("DROP TABLE t");

    // Assert
    assert_eq!(error.sql_state(), "2BP01");
}

#[test]
fn should_drop_a_table_whose_row_type_is_unused() {
    // Arrange
    let fixture = fixture("row_type_drop_unused");

    // Act
    let dropped = fixture.execute("DROP TABLE t");

    // Assert
    assert!(dropped.is_ok());
}

#[test]
fn should_refuse_dropping_a_column_whose_row_type_is_stored_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("row_type_drop_column_in_use");
    fixture.execute("CREATE TABLE u (k int, x t)").unwrap();

    // Act
    let error = fixture.error("ALTER TABLE t DROP COLUMN name");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_link_pg_class_reltype_to_the_pg_type_row() {
    // Arrange
    let fixture = fixture("row_type_catalogs");

    // Act
    let linked = first_column_text(&fixture.rows(
        "SELECT (c.reltype = ty.oid)::text FROM pg_class c \
         JOIN pg_type ty ON ty.typrelid = c.oid \
         WHERE c.relname = 't' AND ty.typtype = 'c'",
    ));

    // Assert
    assert_eq!(linked, vec![Some("true".to_owned())]);
}

#[test]
fn should_allocate_distinct_row_type_oids_below_the_user_range_limit() {
    // Arrange
    let fixture = fixture("row_type_allocated_oids");
    fixture.execute("CREATE TABLE u (k int)").unwrap();

    // Act
    let distinct = first_column_text(&fixture.rows(
        "SELECT (count(DISTINCT reltype) = 2 AND max(reltype) < 2147483648)::text \
         FROM pg_class WHERE relname IN ('t', 'u')",
    ));

    // Assert
    assert_eq!(distinct, vec![Some("true".to_owned())]);
}

#[test]
fn should_resolve_the_row_type_oid_through_regtype() {
    // Arrange
    let fixture = fixture("row_type_regtype");

    // Act
    let name = text(
        &fixture,
        "SELECT reltype::regtype::text FROM pg_class WHERE relname = 't'",
    );

    // Assert
    assert_eq!(name.as_deref(), Some("t"));
}

#[test]
fn should_keep_the_row_type_oid_after_renaming_the_table() {
    // Arrange
    let fixture = fixture("row_type_oid_rename");
    let before = text(
        &fixture,
        "SELECT reltype::text FROM pg_class WHERE relname = 't'",
    );
    fixture.execute("ALTER TABLE t RENAME TO t2").unwrap();

    // Act
    let after = text(
        &fixture,
        "SELECT reltype::text FROM pg_class WHERE relname = 't2'",
    );

    // Assert
    assert_eq!(before, after);
}

#[test]
fn should_refuse_changing_a_column_type_whose_row_type_is_stored_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("row_type_alter_column_type_in_use");
    fixture.execute("CREATE TABLE u (k int, x t)").unwrap();

    // Act
    let error = fixture.error("ALTER TABLE t ALTER COLUMN id TYPE bigint");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_keep_the_column_type_after_a_refused_alter_column_type() {
    // Arrange
    let fixture = fixture("row_type_alter_column_type_kept");
    fixture.execute("CREATE TABLE u (k int, x t)").unwrap();
    let _ = fixture.error("ALTER TABLE t ALTER COLUMN id TYPE bigint");

    // Act
    let id = text(
        &fixture,
        "SELECT data_type FROM information_schema.columns \
         WHERE table_name = 't' AND column_name = 'id'",
    );

    // Assert
    assert_eq!(id.as_deref(), Some("integer"));
}

#[test]
fn should_not_treat_a_cte_named_like_the_table_as_the_table_row() {
    // Arrange
    let fixture = fixture("row_type_cte_shadow");

    // Act
    let value = text(&fixture, "WITH t AS (SELECT 7 AS v) SELECT t.v FROM t");

    // Assert
    assert_eq!(value.as_deref(), Some("7"));
}

#[test]
fn should_reject_a_whole_row_reference_in_a_join_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("row_type_whole_row_join");
    fixture.execute("CREATE TABLE u (k int)").unwrap();

    // Act
    let error = fixture.error("SELECT t FROM t JOIN u ON u.k = t.id");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_reject_a_whole_row_reference_in_group_by_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("row_type_whole_row_group_by");

    // Act
    let error = fixture.error("SELECT count(*) FROM t GROUP BY t");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_reject_a_whole_row_reference_in_a_set_operation_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("row_type_whole_row_union");

    // Act
    let error = fixture.error("SELECT t FROM t UNION ALL SELECT t FROM t");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

fn revision_of(fixture: &SqlFixture, table: &str) -> u64 {
    let suffix = format!(".{table}");
    let name = fixture
        .cassie
        .catalog
        .list_collections()
        .into_iter()
        .map(|collection| collection.name)
        .find(|name| name.ends_with(&suffix))
        .expect("table is registered");
    fixture
        .cassie
        .catalog
        .relation_row_type(&name)
        .expect("table has a row type")
        .revision
}

#[test]
fn should_advance_the_row_type_revision_when_adding_a_column() {
    // Arrange
    let fixture = fixture("row_type_revision_add");
    let before = revision_of(&fixture, "t");

    // Act
    fixture
        .execute("ALTER TABLE t ADD COLUMN extra int")
        .unwrap();

    // Assert
    assert!(revision_of(&fixture, "t") > before);
}

#[test]
fn should_advance_the_row_type_revision_when_dropping_a_column() {
    // Arrange
    let fixture = fixture("row_type_revision_drop");
    let before = revision_of(&fixture, "t");

    // Act
    fixture.execute("ALTER TABLE t DROP COLUMN name").unwrap();

    // Assert
    assert!(revision_of(&fixture, "t") > before);
}

#[test]
fn should_advance_the_row_type_revision_when_renaming_a_column() {
    // Arrange
    let fixture = fixture("row_type_revision_rename");
    let before = revision_of(&fixture, "t");

    // Act
    fixture
        .execute("ALTER TABLE t RENAME COLUMN name TO label")
        .unwrap();

    // Assert
    assert!(revision_of(&fixture, "t") > before);
}

#[test]
fn should_advance_the_row_type_revision_after_add_then_drop() {
    // Arrange
    let fixture = fixture("row_type_revision_add_drop");
    fixture
        .execute("ALTER TABLE t ADD COLUMN extra int")
        .unwrap();
    let after_add = revision_of(&fixture, "t");

    // Act
    fixture.execute("ALTER TABLE t DROP COLUMN extra").unwrap();

    // Assert
    assert!(revision_of(&fixture, "t") > after_add);
}

#[test]
fn should_refuse_changing_a_column_type_whose_row_type_is_nested_in_a_stored_type() {
    // Arrange
    let fixture = fixture("row_type_alter_column_type_nested");
    fixture
        .execute("CREATE TYPE wrap AS (inner_row t)")
        .unwrap();
    fixture.execute("CREATE TABLE u (y wrap)").unwrap();

    // Act
    let error = fixture.error("ALTER TABLE t ALTER COLUMN id TYPE bigint");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

fn join_fixture(label: &str) -> SqlFixture {
    let fixture = fixture(label);
    fixture.execute("CREATE TABLE u (k int, t int)").unwrap();
    fixture.execute("INSERT INTO u VALUES (1, 9)").unwrap();
    fixture
}

#[test]
fn should_read_a_column_named_like_a_joined_table_instead_of_its_row() {
    // Arrange
    let fixture = join_fixture("row_type_join_column_wins");

    // Act
    let value = text(&fixture, "SELECT t FROM t JOIN u ON u.k = t.id");

    // Assert
    assert_eq!(value.as_deref(), Some("9"));
}

#[test]
fn should_group_by_a_column_named_like_a_joined_table() {
    // Arrange
    let fixture = join_fixture("row_type_join_group_by_column");

    // Act
    let value = text(&fixture, "SELECT t FROM t JOIN u ON u.k = t.id GROUP BY t");

    // Assert
    assert_eq!(value.as_deref(), Some("9"));
}

#[test]
fn should_read_a_real_column_in_a_union_branch_named_like_the_first_table() {
    // Arrange
    let fixture = join_fixture("row_type_union_branch_column");

    // Act
    let rows = fixture.rows("SELECT id FROM t UNION SELECT t FROM u");

    // Assert
    assert_eq!(rows.len(), 2);
}

#[test]
fn should_reject_a_union_branch_whole_row_of_another_table_with_sqlstate_0a000() {
    // Arrange
    let fixture = join_fixture("row_type_union_branch_whole_row");

    // Act
    let error = fixture.error("SELECT id FROM t UNION SELECT u FROM u");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_keep_the_row_type_revision_when_a_change_is_rolled_back() {
    // Arrange
    let fixture = fixture("row_type_add_column_rollback");
    fixture.execute("CREATE TABLE u (x t)").unwrap();
    fixture
        .execute("INSERT INTO u VALUES (ROW(2, 'y'))")
        .unwrap();
    let before = revision_of(&fixture, "t");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TABLE t ADD COLUMN extra int")
        .unwrap();

    // Act
    fixture.execute("ROLLBACK").unwrap();

    // Assert
    assert_eq!(revision_of(&fixture, "t"), before);
}

#[test]
fn should_keep_the_embedded_row_type_after_a_rolled_back_add_column() {
    // Arrange
    let fixture = fixture("row_type_rollback_embedded");
    fixture.execute("CREATE TABLE u (x t)").unwrap();
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TABLE t ADD COLUMN extra int")
        .unwrap();
    fixture.execute("ROLLBACK").unwrap();

    // Act
    let inserted = fixture.execute("INSERT INTO u VALUES (ROW(2, 'y'))");
    let rendered = text(&fixture, "SELECT x::text FROM u");

    // Assert
    assert!(inserted.is_ok());
    assert_eq!(rendered.as_deref(), Some("(2,y)"));
}

#[test]
fn should_keep_the_table_attribute_count_after_a_rolled_back_add_column() {
    // Arrange
    let fixture = fixture("row_type_rollback_attributes");
    fixture.execute("BEGIN").unwrap();
    fixture
        .execute("ALTER TABLE t ADD COLUMN extra int")
        .unwrap();
    fixture.execute("ROLLBACK").unwrap();

    // Act
    let count = text(
        &fixture,
        "SELECT count(*)::text FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid WHERE c.relname = 't' AND a.attnum > 0",
    );

    // Assert
    assert_eq!(count.as_deref(), Some("2"));
}

#[test]
fn should_read_a_derived_table_column_named_like_the_joined_table() {
    // Arrange
    let fixture = fixture("row_type_derived_table_column_wins");

    // Act
    let value = text(&fixture, "SELECT t FROM t JOIN (SELECT 1 AS t) d ON true");

    // Assert
    assert_eq!(value.as_deref(), Some("1"));
}
