use super::role_grant_fixture;
use cassie::app::CassieError;
use cassie::types::Value;

#[test]
fn should_filter_unqualified_regclass_by_schema_usage_separately_from_table_select() {
    // Arrange
    let (cassie, root, reader, _projector) = role_grant_fixture();

    // Act
    let hidden_unqualified = cassie
        .execute_sql(
            &reader,
            "SELECT to_regclass('authorization_documents') IS NULL",
            Vec::new(),
        )
        .expect("unqualified lookup should return NULL without schema USAGE");
    // PostgreSQL 18.6 oracle: a qualified name in a schema without USAGE is
    // 42501 "permission denied for schema" (even from to_regclass), while
    // to_regnamespace needs no USAGE.
    let qualified_without_usage = cassie
        .execute_sql(
            &reader,
            "SELECT to_regnamespace('public') IS NOT NULL",
            Vec::new(),
        )
        .expect("namespace lookups should not require schema USAGE");
    let qualified_regclass_without_usage = cassie.execute_sql(
        &reader,
        "SELECT to_regclass('public.authorization_documents') IS NOT NULL",
        Vec::new(),
    );
    let hidden_regclass_cast = cassie.execute_sql(
        &reader,
        "SELECT 'authorization_documents'::regclass IS NOT NULL",
        Vec::new(),
    );

    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant reader schema usage");
    let visible_without_select = cassie
        .execute_sql(
            &reader,
            "SELECT to_regclass('authorization_documents') IS NOT NULL, \
                    'authorization_documents'::regclass IS NOT NULL",
            Vec::new(),
        )
        .expect("unqualified identity lookups should resolve after schema USAGE");
    let denied_relation_read = cassie.execute_sql(
        &reader,
        "SELECT body FROM public.authorization_documents WHERE id = 1",
        Vec::new(),
    );

    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant reader table SELECT");
    let allowed_relation_read = cassie
        .execute_sql(
            &reader,
            "SELECT body FROM public.authorization_documents WHERE id = 1",
            Vec::new(),
        )
        .expect("table SELECT should work after its separate grant");

    // Assert
    assert_eq!(hidden_unqualified.rows(), vec![vec![Value::Bool(true)]]);
    assert_eq!(
        qualified_without_usage.rows(),
        vec![vec![Value::Bool(true)]]
    );
    assert_eq!(
        qualified_regclass_without_usage
            .expect_err("qualified lookup needs schema USAGE")
            .sql_state(),
        "42501"
    );
    assert!(
        hidden_regclass_cast.is_err(),
        "unqualified regclass cast should fail while its schema is hidden"
    );
    assert_eq!(
        visible_without_select.rows(),
        vec![vec![Value::Bool(true), Value::Bool(true)]]
    );
    assert!(matches!(
        denied_relation_read,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert_eq!(
        allowed_relation_read.rows(),
        vec![vec![Value::String("original".to_owned())]]
    );
}

#[test]
fn should_filter_unqualified_routine_lookups_by_schema_usage() {
    // Arrange
    // PostgreSQL 18.6 oracle: without USAGE on the schema, an unqualified
    // to_regprocedure/to_regtype is NULL and an unqualified cast is undefined.
    let (cassie, root, reader, _projector) = role_grant_fixture();
    cassie
        .execute_sql(
            &root,
            "CREATE FUNCTION public.visible_fn(x INT) RETURNS INT AS \"x + 1\"",
            Vec::new(),
        )
        .expect("create function");
    cassie
        .execute_sql(
            &root,
            "CREATE TYPE public.visible_pair AS (left_side INT)",
            Vec::new(),
        )
        .expect("create type");
    let lookups = "SELECT to_regprocedure('visible_fn(integer)') IS NULL, \
                          to_regtype('visible_pair') IS NULL";

    // Act
    let hidden = cassie
        .execute_sql(&reader, lookups, Vec::new())
        .expect("hidden lookups return NULL");
    let hidden_procedure_cast = cassie.execute_sql(
        &reader,
        "SELECT 'visible_fn(integer)'::regprocedure IS NOT NULL",
        Vec::new(),
    );
    let hidden_type_cast = cassie.execute_sql(
        &reader,
        "SELECT 'visible_pair'::regtype IS NOT NULL",
        Vec::new(),
    );
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant reader schema usage");
    let visible = cassie
        .execute_sql(&reader, lookups, Vec::new())
        .expect("visible lookups");
    let visible_casts = cassie
        .execute_sql(
            &reader,
            "SELECT 'visible_fn(integer)'::regprocedure IS NOT NULL, \
                    'visible_pair'::regtype IS NOT NULL",
            Vec::new(),
        )
        .expect("visible casts");

    // Assert
    assert_eq!(
        hidden.rows(),
        vec![vec![Value::Bool(true), Value::Bool(true)]]
    );
    assert!(hidden_procedure_cast.is_err());
    assert!(hidden_type_cast.is_err());
    assert_eq!(
        visible.rows(),
        vec![vec![Value::Bool(false), Value::Bool(false)]]
    );
    assert_eq!(
        visible_casts.rows(),
        vec![vec![Value::Bool(true), Value::Bool(true)]]
    );
}

/// Runs each denied statement as `reader` before and the allowed statement
/// after `GRANT USAGE`, returning the SQLSTATEs and the post-grant rows.
fn qualified_lookup(denied: &[&str], allowed: &str) -> (Vec<String>, Vec<Vec<Value>>) {
    let (cassie, root, reader, _projector) = role_grant_fixture();
    for statement in [
        "CREATE FUNCTION public.visible_fn(x INT) RETURNS INT AS \"x + 1\"",
        "CREATE TYPE public.visible_pair AS (left_side INT)",
    ] {
        cassie
            .execute_sql(&root, statement, Vec::new())
            .expect("create object");
    }
    let states = denied
        .iter()
        .map(|sql| {
            cassie
                .execute_sql(&reader, sql, Vec::new())
                .expect_err("qualified lookup needs schema USAGE")
                .sql_state()
                .to_owned()
        })
        .collect();
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant reader schema usage");
    let rows = cassie
        .execute_sql(&reader, allowed, Vec::new())
        .expect("qualified lookup resolves with schema USAGE")
        .rows()
        .to_vec();
    (states, rows)
}

// PostgreSQL 18.6 oracle for the four tests below: a qualified to_reg* or cast
// in a schema without USAGE raises 42501 (even for a missing object);
// to_regnamespace needs no USAGE.
#[test]
fn should_require_schema_usage_for_qualified_regclass() {
    // Arrange
    let denied = [
        "SELECT to_regclass('public.authorization_documents')",
        "SELECT 'public.authorization_documents'::regclass",
        "SELECT to_regclass('public.no_such_relation')",
    ];

    // Act
    let (states, rows) = qualified_lookup(
        &denied,
        "SELECT to_regclass('public.authorization_documents') IS NOT NULL, \
                to_regclass('public.no_such_relation') IS NULL",
    );

    // Assert
    assert_eq!(states, vec!["42501"; denied.len()]);
    assert_eq!(rows, vec![vec![Value::Bool(true), Value::Bool(true)]]);
}

#[test]
fn should_require_schema_usage_for_qualified_regtype() {
    // Arrange
    let denied = [
        "SELECT to_regtype('public.visible_pair')",
        "SELECT 'public.visible_pair'::regtype",
    ];

    // Act
    let (states, rows) =
        qualified_lookup(&denied, "SELECT 'public.visible_pair'::regtype IS NOT NULL");

    // Assert
    assert_eq!(states, vec!["42501"; denied.len()]);
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_require_schema_usage_for_qualified_regprocedure() {
    // Arrange
    let denied = [
        "SELECT to_regprocedure('public.visible_fn(integer)')",
        "SELECT 'public.visible_fn(integer)'::regprocedure",
    ];

    // Act
    let (states, rows) = qualified_lookup(
        &denied,
        "SELECT to_regprocedure('public.visible_fn(integer)') IS NOT NULL",
    );

    // Assert
    assert_eq!(states, vec!["42501"; denied.len()]);
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_not_require_schema_usage_for_regnamespace() {
    // Arrange
    let (cassie, _root, reader, _projector) = role_grant_fixture();

    // Act
    let rows = cassie
        .execute_sql(
            &reader,
            "SELECT to_regnamespace('public') IS NOT NULL, 'public'::regnamespace IS NOT NULL",
            Vec::new(),
        )
        .expect("regnamespace needs no schema USAGE")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true), Value::Bool(true)]]);
}

#[test]
fn should_let_root_resolve_qualified_names_without_a_usage_grant() {
    // Arrange
    let (cassie, root, _reader, _projector) = role_grant_fixture();

    // Act
    let rows = cassie
        .execute_sql(
            &root,
            "SELECT to_regclass('public.authorization_documents') IS NOT NULL",
            Vec::new(),
        )
        .expect("root bypasses the schema USAGE check")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

/// The SQLSTATE each lookup raises for `reader`, which has no `USAGE` on `public`.
fn reader_lookup_states<const N: usize>(lookups: [&str; N]) -> [String; N] {
    let (cassie, _root, reader, _projector) = role_grant_fixture();
    lookups.map(|lookup| {
        cassie
            .execute_sql(&reader, &format!("SELECT {lookup} IS NOT NULL"), Vec::new())
            .map_or_else(
                |error| error.sql_state().to_string(),
                |_| "resolved".to_string(),
            )
    })
}

#[test]
fn should_require_schema_usage_when_white_space_surrounds_the_dot_of_a_relation_name() {
    // Arrange
    // PostgreSQL 18.6 oracle: white space around the dot is insignificant, so
    // each spelling is `public.authorization_documents` and raises 42501.
    let lookups = [
        "to_regclass('public . authorization_documents')",
        "to_regclass('public .authorization_documents')",
        "to_regclass('public. authorization_documents')",
        "'public . authorization_documents'::regclass",
    ];

    // Act
    let states = reader_lookup_states(lookups);

    // Assert
    assert_eq!(states, ["42501", "42501", "42501", "42501"]);
}

#[test]
fn should_require_schema_usage_for_a_spaced_name_with_a_quoted_schema() {
    // Arrange
    let lookups = ["to_regclass('\"public\" .authorization_documents')"];

    // Act
    let states = reader_lookup_states(lookups);

    // Assert
    assert_eq!(states, ["42501"]);
}

#[test]
fn should_require_schema_usage_for_a_spaced_database_qualified_name() {
    // Arrange
    let lookups = ["to_regclass('postgres . public . authorization_documents')"];

    // Act
    let states = reader_lookup_states(lookups);

    // Assert
    assert_eq!(states, ["42501"]);
}

#[test]
fn should_require_schema_usage_for_a_spaced_type_name() {
    // Arrange
    let lookups = ["to_regtype('public . authorization_documents')"];

    // Act
    let states = reader_lookup_states(lookups);

    // Assert
    assert_eq!(states, ["42501"]);
}

#[test]
fn should_require_schema_usage_for_a_spaced_routine_name() {
    // Arrange
    let lookups = [
        "to_regproc('public . lookup')",
        "to_regprocedure('public . lookup(integer)')",
    ];

    // Act
    let states = reader_lookup_states(lookups);

    // Assert
    assert_eq!(states, ["42501", "42501"]);
}
