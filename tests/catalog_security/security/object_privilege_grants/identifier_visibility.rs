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
