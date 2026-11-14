//! Enum values: literals, coercion, ordering, grouping, operators, helper
//! functions and arrays. Declared order (low < medium < high) differs from
//! alphabetical order (high < low < medium) so ordering mistakes are visible.
//! Authored against the PostgreSQL 18.6 oracle.

use crate::support_enum_types::{scalar, strings};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE priority AS ENUM ('low', 'medium', 'high')",
            "CREATE TABLE tasks (id int PRIMARY KEY, p priority)",
            "INSERT INTO tasks VALUES (1, 'medium'), (2, 'high'), (3, NULL), (4, 'low'), (5, 'high')",
        ],
    )
}

#[test]
fn should_accept_every_literal_form_on_writes() {
    // Arrange
    let fixture = fixture("enum_values_write_coercion");

    // Act
    fixture
        .execute("INSERT INTO tasks VALUES (6, 'low'::priority), (7, CAST('medium' AS priority))")
        .unwrap();
    fixture
        .execute("UPDATE tasks SET p = 'high' WHERE id = 6")
        .unwrap();
    fixture
        .execute("UPDATE tasks SET p = 'low'::priority WHERE id = 7")
        .unwrap();
    let rows = strings(
        &fixture,
        "SELECT id::text || '=' || p::text FROM tasks WHERE id >= 6 ORDER BY id",
    );

    // Assert
    assert_eq!(rows, vec!["6=high", "7=low"]);
}

#[test]
fn should_coerce_unknown_literals_in_comparisons() {
    // Arrange
    let fixture = fixture("enum_values_read_coercion");

    // Act
    let equal = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p = 'high' ORDER BY id",
    );
    let listed = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p IN ('low', 'medium') ORDER BY id",
    );
    let any_literal = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p = ANY('{low,high}') ORDER BY id",
    );
    let any_array = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p = ANY (ARRAY['low','high']::priority[]) ORDER BY id",
    );

    // Assert
    assert_eq!(equal, vec!["2", "5"]);
    assert_eq!(listed, vec!["1", "4"]);
    assert_eq!(any_literal, vec!["2", "4", "5"]);
    assert_eq!(any_array, vec!["2", "4", "5"]);
}

#[test]
fn should_coerce_unknown_literals_in_values_lists() {
    // Arrange
    let fixture = fixture("enum_values_values_list");

    // Act
    let rows = strings(
        &fixture,
        "SELECT t.id::text FROM tasks t JOIN (VALUES ('low'::priority), ('high')) v(p) \
         ON t.p = v.p ORDER BY t.id",
    );

    // Assert
    assert_eq!(rows, vec!["2", "4", "5"]);
}

#[test]
fn should_reject_an_invalid_label_with_sqlstate_22p02() {
    // Arrange
    let fixture = fixture("enum_values_invalid_label");

    // Act
    let cast = fixture.error("SELECT 'urgent'::priority");
    let insert = fixture.error("INSERT INTO tasks VALUES (8, 'urgent')");
    let update = fixture.error("UPDATE tasks SET p = 'urgent'");
    let comparison = fixture.error("SELECT id FROM tasks WHERE p = 'urgent'");
    let in_list = fixture.error("SELECT id FROM tasks WHERE p IN ('low', 'urgent')");
    let any_literal = fixture.error("SELECT id FROM tasks WHERE p = ANY('{low,urgent}')");
    let array_literal = fixture.error("SELECT '{low,urgent}'::priority[]");

    // Assert
    for error in [
        cast,
        insert,
        update,
        comparison,
        in_list,
        any_literal,
        array_literal,
    ] {
        assert_eq!(error.sql_state(), "22P02");
    }
}

#[test]
fn should_reject_an_invalid_label_even_when_no_row_reaches_the_comparison() {
    // Arrange
    let fixture = fixture("enum_values_invalid_label_without_rows");
    fixture
        .execute("CREATE TABLE empty_tasks (id int PRIMARY KEY, p priority)")
        .unwrap();

    // Act
    let empty_table = fixture.error("SELECT id FROM empty_tasks WHERE p = 'urgent'");
    let in_list = fixture.error("SELECT id FROM empty_tasks WHERE p IN ('low', 'urgent')");
    let any_literal = fixture.error("SELECT id FROM empty_tasks WHERE p = ANY('{low,urgent}')");
    let between = fixture.error("SELECT id FROM empty_tasks WHERE p BETWEEN 'low' AND 'urgent'");
    let short_circuited = fixture.error("SELECT id FROM tasks WHERE false AND p = 'urgent'");

    // Assert
    for error in [empty_table, in_list, any_literal, between, short_circuited] {
        assert_eq!(error.sql_state(), "22P02");
    }
}

#[test]
fn should_match_labels_case_sensitively() {
    // Arrange
    let fixture = fixture("enum_values_case");

    // Act
    let error = fixture.error("SELECT 'High'::priority");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
}

#[test]
fn should_order_by_declared_order_with_nulls_last_by_default() {
    // Arrange
    let fixture = fixture("enum_values_order_by");

    // Act
    let ascending = strings(&fixture, "SELECT id::text FROM tasks ORDER BY p, id");
    let descending = strings(&fixture, "SELECT id::text FROM tasks ORDER BY p DESC, id");
    let nulls_first = strings(
        &fixture,
        "SELECT id::text FROM tasks ORDER BY p NULLS FIRST, id",
    );
    let descending_nulls_last = strings(
        &fixture,
        "SELECT id::text FROM tasks ORDER BY p DESC NULLS LAST, id",
    );

    // Assert
    assert_eq!(ascending, vec!["4", "1", "2", "5", "3"]);
    assert_eq!(descending, vec!["3", "2", "5", "1", "4"]);
    assert_eq!(nulls_first, vec!["3", "4", "1", "2", "5"]);
    assert_eq!(descending_nulls_last, vec!["2", "5", "1", "4", "3"]);
}

#[test]
fn should_group_by_enum_values_in_declared_order() {
    // Arrange
    let fixture = fixture("enum_values_group_by");

    // Act
    let groups = strings(
        &fixture,
        "SELECT coalesce(p::text, 'NULL') || ':' || count(*)::text FROM tasks \
         GROUP BY p ORDER BY p",
    );

    // Assert
    assert_eq!(groups, vec!["low:1", "medium:1", "high:2", "NULL:1"]);
}

#[test]
fn should_return_distinct_enum_values() {
    // Arrange
    let fixture = fixture("enum_values_distinct");

    // Act
    let distinct = strings(
        &fixture,
        "SELECT p::text AS label FROM (SELECT DISTINCT p FROM tasks) d ORDER BY p",
    );

    // Assert
    assert_eq!(distinct, vec!["low", "medium", "high", "NULL"]);
}

#[test]
fn should_aggregate_extremes_by_declared_order() {
    // Arrange
    let fixture = fixture("enum_values_min_max");

    // Act
    let extremes = scalar(
        &fixture,
        "SELECT min(p)::text || '/' || max(p)::text FROM tasks",
    );
    let unnested = scalar(
        &fixture,
        "SELECT min(v)::text || '/' || max(v)::text \
         FROM unnest(ARRAY['medium','low','high']::priority[]) v",
    );

    // Assert
    assert_eq!(extremes.as_deref(), Some("low/high"));
    assert_eq!(unnested.as_deref(), Some("low/high"));
}

#[test]
fn should_compare_with_every_operator_by_sort_order() {
    // Arrange
    let fixture = fixture("enum_values_operators");

    // Act
    let greater = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p > 'low' ORDER BY id",
    );
    let at_most = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p <= 'medium' ORDER BY id",
    );
    let below = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p < 'medium' ORDER BY id",
    );
    let not_equal = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p <> 'high' ORDER BY id",
    );
    let at_least = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p >= 'medium' ORDER BY id",
    );
    let between = strings(
        &fixture,
        "SELECT id::text FROM tasks WHERE p BETWEEN 'medium' AND 'high' ORDER BY id",
    );

    // Assert
    assert_eq!(greater, vec!["1", "2", "5"]);
    assert_eq!(at_most, vec!["1", "4"]);
    assert_eq!(below, vec!["4"]);
    assert_eq!(
        not_equal,
        vec!["1", "4"],
        "NULL is neither equal nor unequal"
    );
    assert_eq!(at_least, vec!["1", "2", "5"]);
    assert_eq!(between, vec!["1", "2", "5"]);
}

#[test]
fn should_compare_typed_literals_by_declared_order_not_text() {
    // Arrange
    let fixture = fixture("enum_values_literal_compare");

    // Act
    let result = scalar(
        &fixture,
        "SELECT ('low'::priority < 'high'::priority)::text || ('high'::priority < 'low'::priority)::text",
    );

    // Assert
    assert_eq!(result.as_deref(), Some("truefalse"));
}

#[test]
fn should_reject_comparing_an_enum_with_a_text_operand_with_sqlstate_42883() {
    // Arrange
    let fixture = fixture("enum_values_text_compare");

    // Act
    let typed_text = fixture.error("SELECT 'low'::priority = 'low'::text");
    let varchar = fixture.error("SELECT 'low'::priority = 'low'::varchar");
    let column = fixture.error("SELECT id FROM tasks WHERE p = 'low'::text");

    // Assert
    assert_eq!(typed_text.sql_state(), "42883");
    assert_eq!(varchar.sql_state(), "42883");
    assert_eq!(column.sql_state(), "42883");
}

#[test]
fn should_reject_comparing_two_enum_types_with_sqlstate_42883() {
    // Arrange
    let fixture = fixture("enum_values_two_types");
    fixture
        .execute("CREATE TYPE level AS ENUM ('low')")
        .unwrap();

    // Act
    let error = fixture.error("SELECT 'low'::level = 'low'::priority");

    // Assert
    assert_eq!(error.sql_state(), "42883");
}

#[test]
fn should_reject_integer_casts_with_sqlstate_42846() {
    // Arrange
    let fixture = fixture("enum_values_int_cast");

    // Act
    let to_int = fixture.error("SELECT 'low'::priority::int");
    let from_int = fixture.error("SELECT 1::priority");
    let to_bigint = fixture.error("SELECT 'low'::priority::bigint");

    // Assert
    assert_eq!(to_int.sql_state(), "42846");
    assert_eq!(from_int.sql_state(), "42846");
    assert_eq!(to_bigint.sql_state(), "42846");
}

#[test]
fn should_reject_casting_one_enum_to_another_with_sqlstate_42846() {
    // Arrange
    let fixture = fixture("enum_values_enum_to_enum");
    fixture
        .execute("CREATE TYPE level AS ENUM ('low')")
        .unwrap();

    // Act
    let error = fixture.error("SELECT 'low'::level::priority");

    // Assert
    assert_eq!(error.sql_state(), "42846");
}

#[test]
fn should_cast_an_enum_through_text_in_either_direction() {
    // Arrange
    let fixture = fixture("enum_values_text_cast");

    // Act
    let to_text = scalar(&fixture, "SELECT 'high'::priority::text || 'x'");
    let to_varchar = scalar(&fixture, "SELECT 'high'::priority::varchar");
    let from_text = scalar(&fixture, "SELECT 'medium'::text::priority::text");
    let bad_text = fixture.error("SELECT 'bogus'::text::priority");

    // Assert
    assert_eq!(to_text.as_deref(), Some("highx"));
    assert_eq!(to_varchar.as_deref(), Some("high"));
    assert_eq!(from_text.as_deref(), Some("medium"));
    assert_eq!(bad_text.sql_state(), "22P02");
}

#[test]
fn should_report_enum_expression_types_through_pg_typeof() {
    // Arrange
    let fixture = fixture("enum_values_pg_typeof");

    // Act
    let scalar_type = scalar(&fixture, "SELECT pg_typeof('low'::priority)::text");
    let array_type = scalar(
        &fixture,
        "SELECT pg_typeof(ARRAY['low','high']::priority[])::text",
    );

    // Assert
    assert_eq!(scalar_type.as_deref(), Some("priority"));
    assert_eq!(array_type.as_deref(), Some("priority[]"));
}

#[test]
fn should_return_enum_support_functions_in_declared_order() {
    // Arrange
    let fixture = fixture("enum_values_helpers");

    // Act
    let all = scalar(&fixture, "SELECT enum_range(NULL::priority)::text");
    let first = scalar(&fixture, "SELECT enum_first(NULL::priority)::text");
    let last = scalar(&fixture, "SELECT enum_last('low'::priority)::text");
    let bounded = scalar(
        &fixture,
        "SELECT enum_range('medium'::priority, 'high')::text",
    );
    let open_lower = scalar(
        &fixture,
        "SELECT enum_range(NULL, 'medium'::priority)::text",
    );
    let open_upper = scalar(
        &fixture,
        "SELECT enum_range('medium'::priority, NULL)::text",
    );
    let inverted = scalar(&fixture, "SELECT enum_range('high'::priority, 'low')::text");

    // Assert
    assert_eq!(all.as_deref(), Some("{low,medium,high}"));
    assert_eq!(first.as_deref(), Some("low"));
    assert_eq!(last.as_deref(), Some("high"));
    assert_eq!(bounded.as_deref(), Some("{medium,high}"));
    assert_eq!(open_lower.as_deref(), Some("{low,medium}"));
    assert_eq!(open_upper.as_deref(), Some("{medium,high}"));
    assert_eq!(inverted.as_deref(), Some("{}"));
}

#[test]
fn should_reject_enum_boundaries_on_an_empty_enum_with_sqlstate_55000() {
    // Arrange
    let fixture = fixture("enum_values_empty_helpers");
    fixture.execute("CREATE TYPE nothing AS ENUM ()").unwrap();

    // Act
    let first = fixture.error("SELECT enum_first(NULL::nothing)");
    let last = fixture.error("SELECT enum_last(NULL::nothing)");
    let range = scalar(&fixture, "SELECT enum_range(NULL::nothing)::text");

    // Assert
    assert_eq!(first.sql_state(), "55000");
    assert_eq!(last.sql_state(), "55000");
    assert_eq!(
        range.as_deref(),
        Some("{}"),
        "an empty range is not an error"
    );
}

#[test]
fn should_follow_added_values_in_enum_range_by_sort_order() {
    // Arrange
    let fixture = fixture("enum_values_range_after_add");
    fixture
        .execute("ALTER TYPE priority ADD VALUE 'urgent'")
        .unwrap();
    fixture
        .execute("ALTER TYPE priority ADD VALUE 'normal' BEFORE 'medium'")
        .unwrap();

    // Act
    let all = scalar(&fixture, "SELECT enum_range(NULL::priority)::text");
    let last = scalar(&fixture, "SELECT enum_last(NULL::priority)::text");

    // Assert
    assert_eq!(all.as_deref(), Some("{low,normal,medium,high,urgent}"));
    assert_eq!(last.as_deref(), Some("urgent"));
}

#[test]
fn should_round_trip_enum_array_columns() {
    // Arrange
    let fixture = fixture("enum_values_array_column");
    fixture
        .execute("CREATE TABLE boards (id int PRIMARY KEY, ps priority[])")
        .unwrap();
    fixture
        .execute(
            "INSERT INTO boards VALUES (1, '{low,high}'), (2, ARRAY['high','medium']::priority[]), \
             (3, NULL)",
        )
        .unwrap();

    // Act
    let rendered = strings(
        &fixture,
        "SELECT coalesce(ps::text, 'NULL') FROM boards ORDER BY id",
    );
    let element = scalar(&fixture, "SELECT ps[2]::text FROM boards WHERE id = 1");
    let membership = strings(
        &fixture,
        "SELECT id::text FROM boards WHERE 'high' = ANY(ps) ORDER BY id",
    );
    let invalid = fixture.error("INSERT INTO boards VALUES (4, '{low,urgent}')");

    // Assert
    assert_eq!(rendered, vec!["{low,high}", "{high,medium}", "NULL"]);
    assert_eq!(element.as_deref(), Some("high"));
    assert_eq!(membership, vec!["1", "2"]);
    assert_eq!(invalid.sql_state(), "22P02");
}

#[test]
fn should_order_an_unnested_enum_array_by_declared_order() {
    // Arrange
    let fixture = fixture("enum_values_array_unnest");

    // Act
    let ordered = strings(
        &fixture,
        "SELECT v::text AS label FROM unnest(ARRAY['high','low','medium']::priority[]) v ORDER BY v",
    );
    let any_match = scalar(
        &fixture,
        "SELECT ('medium'::priority = ANY (ARRAY['low','medium']::priority[]))::text",
    );
    let literal = scalar(&fixture, "SELECT '{low,high}'::priority[]::text");
    let type_name = scalar(&fixture, "SELECT pg_typeof(ARRAY['low']::priority[])::text");

    // Assert
    assert_eq!(ordered, vec!["low", "medium", "high"]);
    assert_eq!(any_match.as_deref(), Some("true"));
    assert_eq!(literal.as_deref(), Some("{low,high}"));
    assert_eq!(type_name.as_deref(), Some("priority[]"));
}
