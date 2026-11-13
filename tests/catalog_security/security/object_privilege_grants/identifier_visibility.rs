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
    let qualified_without_usage = cassie
        .execute_sql(
            &reader,
            "SELECT to_regclass('public.authorization_documents') IS NOT NULL, \
                    to_regnamespace('public') IS NOT NULL",
            Vec::new(),
        )
        .expect("qualified identity lookups should not require schema USAGE");
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
        vec![vec![Value::Bool(true), Value::Bool(true)]]
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
