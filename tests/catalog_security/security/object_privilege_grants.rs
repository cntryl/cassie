use cassie::app::{Cassie, CassieError, CassieSession, CatalogObjectKind};
use cassie::config::CassieRuntimeConfig;
use cassie::sql;
use cassie::types::Value;

fn cassie() -> Cassie {
    let config = crate::support_sql::with_test_root_password(CassieRuntimeConfig::default());
    let cassie = Cassie::new_in_memory_with_config(config).expect("in-memory Cassie");
    cassie.startup().expect("startup");
    cassie
}

fn root_session(cassie: &Cassie) -> CassieSession {
    cassie
        .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
        .expect("root login")
}

fn create_table_and_role(cassie: &Cassie, root: &CassieSession, role: &str) -> CassieSession {
    cassie
        .execute_sql(
            root,
            "CREATE TABLE public.authorization_documents (id INTEGER PRIMARY KEY, body TEXT)",
            Vec::new(),
        )
        .expect("create table");
    cassie
        .execute_sql(
            root,
            "INSERT INTO public.authorization_documents VALUES (1, 'original')",
            Vec::new(),
        )
        .expect("insert fixture row");
    cassie
        .execute_sql(
            root,
            &format!("CREATE ROLE {role} LOGIN PASSWORD '{role}-secret'"),
            Vec::new(),
        )
        .expect("create login role");
    cassie
        .authenticate_role(
            role,
            Some(&format!("{role}-secret")),
            Some("postgres".to_owned()),
        )
        .expect("role login")
}

#[test]
fn should_parse_selected_schema_table_privilege_grant_forms() {
    // Arrange
    let statements = [
        "GRANT USAGE ON SCHEMA public TO reader",
        "REVOKE USAGE ON SCHEMA public FROM reader",
        "GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.authorization_documents TO projector",
        "REVOKE SELECT, INSERT, UPDATE, DELETE ON TABLE public.authorization_documents FROM projector",
    ];
    let unsupported = [
        "GRANT ALL ON TABLE public.authorization_documents TO reader",
        "GRANT TRUNCATE ON TABLE public.authorization_documents TO reader",
        "GRANT EXECUTE ON FUNCTION public.lookup(INTEGER) TO reader",
        "GRANT SELECT ON ALL TABLES IN SCHEMA public TO reader",
        "GRANT SELECT ON TABLE public.authorization_documents TO PUBLIC",
        "GRANT SELECT ON TABLE public.authorization_documents TO reader WITH GRANT OPTION",
    ];

    // Act
    let parsed = statements.map(sql::parse_statement);
    let rejected = unsupported.map(sql::parse_statement);

    // Assert
    assert!(parsed.iter().all(Result::is_ok));
    assert!(rejected.iter().all(Result::is_err));
}

fn role_grant_fixture() -> (Cassie, CassieSession, CassieSession, CassieSession) {
    let cassie = cassie();
    let root = root_session(&cassie);
    let reader = create_table_and_role(&cassie, &root, "reader");
    cassie
        .execute_sql(
            &root,
            "CREATE ROLE projector LOGIN PASSWORD 'projector-secret'",
            Vec::new(),
        )
        .expect("create projector role");
    let projector = cassie
        .authenticate_role(
            "projector",
            Some("projector-secret"),
            Some("postgres".to_owned()),
        )
        .expect("projector login");
    (cassie, root, reader, projector)
}

#[test]
fn should_require_schema_usage_before_reader_relation_lookup() {
    // Arrange
    let (cassie, _root, reader, _projector) = role_grant_fixture();

    // Act
    let denied = cassie.execute_sql(
        &reader,
        "SELECT body FROM public.authorization_documents",
        Vec::new(),
    );

    // Assert
    assert!(matches!(denied, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_apply_reader_select_privilege_at_query_execution() {
    // Arrange
    let (cassie, root, reader, _projector) = role_grant_fixture();
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant reader schema usage");

    // Act
    let denied_direct = cassie.execute_sql(
        &reader,
        "SELECT body FROM public.authorization_documents",
        Vec::new(),
    );
    cassie
        .execute_sql(
            &reader,
            "PREPARE unauthorized_read AS SELECT body FROM public.authorization_documents",
            Vec::new(),
        )
        .expect("prepare is session-local and does not read rows");
    let denied_prepared = cassie.execute_sql(&reader, "EXECUTE unauthorized_read", Vec::new());

    // Assert
    assert!(matches!(
        denied_direct,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert!(matches!(
        denied_prepared,
        Err(CassieError::InsufficientPrivilege)
    ));
}

#[test]
fn should_deny_reader_mutations_without_table_privileges() {
    // Arrange
    let (cassie, root, reader, _projector) = role_grant_fixture();
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant reader schema usage");

    // Act
    let denied_insert = cassie.execute_sql(
        &reader,
        "INSERT INTO public.authorization_documents VALUES (2, 'denied')",
        Vec::new(),
    );
    let denied_update = cassie.execute_sql(
        &reader,
        "UPDATE public.authorization_documents SET body = 'denied' WHERE id = 1",
        Vec::new(),
    );
    let denied_delete = cassie.execute_sql(
        &reader,
        "DELETE FROM public.authorization_documents WHERE id = 1",
        Vec::new(),
    );
    let denied_copy = cassie.execute_sql(
        &reader,
        "COPY public.authorization_documents FROM STDIN",
        Vec::new(),
    );

    // Assert
    for denied in [denied_insert, denied_update, denied_delete, denied_copy] {
        assert!(matches!(denied, Err(CassieError::InsufficientPrivilege)));
    }
}

#[test]
fn should_allow_reader_queries_with_explicit_grants() {
    // Arrange
    let (cassie, root, reader, _projector) = role_grant_fixture();
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant reader schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant reader select");

    // Act
    let selected = cassie
        .execute_sql(
            &reader,
            "SELECT body FROM public.authorization_documents WHERE id = 1",
            Vec::new(),
        )
        .expect("reader select");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("original".to_owned())]]
    );
}

#[test]
fn should_allow_projector_writes_with_explicit_grants() {
    // Arrange
    let (cassie, root, _reader, projector) = role_grant_fixture();
    cassie
        .execute_sql(
            &root,
            "GRANT USAGE ON SCHEMA public TO projector",
            Vec::new(),
        )
        .expect("grant projector schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.authorization_documents TO projector",
            Vec::new(),
        )
        .expect("grant projector access");

    // Act
    let insert = cassie.execute_sql(
        &projector,
        "INSERT INTO public.authorization_documents VALUES (2, 'projected')",
        Vec::new(),
    );

    // Assert
    assert!(insert.is_ok());
}

#[test]
fn should_reject_implicit_sequence_defaults_for_non_admin_insert() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    cassie
        .execute_sql(
            &root,
            "CREATE TABLE public.sequence_default_documents (id SERIAL PRIMARY KEY, body TEXT)",
            Vec::new(),
        )
        .expect("create sequence-defaulted table");
    cassie
        .execute_sql(
            &root,
            "CREATE ROLE sequence_writer LOGIN PASSWORD 'sequence-writer-secret'",
            Vec::new(),
        )
        .expect("create writer role");
    let writer = cassie
        .authenticate_role(
            "sequence_writer",
            Some("sequence-writer-secret"),
            Some("postgres".to_owned()),
        )
        .expect("authenticate writer");
    cassie
        .execute_sql(
            &root,
            "GRANT USAGE ON SCHEMA public TO sequence_writer",
            Vec::new(),
        )
        .expect("grant schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT INSERT ON TABLE public.sequence_default_documents TO sequence_writer",
            Vec::new(),
        )
        .expect("grant table insert");
    let sequence_name = cassie
        .catalog
        .get_constraints("public.sequence_default_documents")
        .into_iter()
        .find(|constraint| constraint.field == "id")
        .and_then(|constraint| constraint.default_sequence)
        .expect("serial sequence default");
    let sequence_before = cassie
        .midge
        .get_sequence(&sequence_name)
        .expect("read sequence before insert")
        .expect("sequence metadata");

    // Act
    let implicit_default = cassie.execute_sql(
        &writer,
        "INSERT INTO public.sequence_default_documents (body) VALUES ('denied')",
        Vec::new(),
    );
    let explicit_value = cassie.execute_sql(
        &writer,
        "INSERT INTO public.sequence_default_documents (id, body) VALUES (9, 'explicit')",
        Vec::new(),
    );
    let rows = cassie
        .execute_sql(
            &root,
            "SELECT id FROM public.sequence_default_documents",
            Vec::new(),
        )
        .expect("inspect inserted rows");
    let sequence_after = cassie
        .midge
        .get_sequence(&sequence_name)
        .expect("read sequence after insert")
        .expect("sequence metadata");

    // Assert
    assert!(matches!(
        implicit_default,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert!(explicit_value.is_ok());
    assert_eq!(rows.rows(), vec![vec![Value::Int64(9)]]);
    assert_eq!(sequence_after.current_value, sequence_before.current_value);
    assert_eq!(sequence_after.is_called, sequence_before.is_called);
}

#[test]
fn should_reject_other_non_admin_writes_that_consume_sequence_defaults() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    cassie
        .execute_sql(
            &root,
            "CREATE TABLE public.sequence_default_write_paths (id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, body TEXT)",
            Vec::new(),
        )
        .expect("create sequence-defaulted table");
    cassie
        .execute_sql(
            &root,
            "INSERT INTO public.sequence_default_write_paths (id, body) VALUES (9, 'existing')",
            Vec::new(),
        )
        .expect("insert explicit root row");
    cassie
        .execute_sql(
            &root,
            "CREATE ROLE sequence_writer LOGIN PASSWORD 'sequence-writer-secret'",
            Vec::new(),
        )
        .expect("create writer role");
    let writer = cassie
        .authenticate_role(
            "sequence_writer",
            Some("sequence-writer-secret"),
            Some("postgres".to_owned()),
        )
        .expect("authenticate writer");
    cassie
        .execute_sql(
            &root,
            "GRANT USAGE ON SCHEMA public TO sequence_writer",
            Vec::new(),
        )
        .expect("grant schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT, INSERT, UPDATE ON TABLE public.sequence_default_write_paths TO sequence_writer",
            Vec::new(),
        )
        .expect("grant selected table access");
    let sequence_name = cassie
        .catalog
        .get_constraints("public.sequence_default_write_paths")
        .into_iter()
        .find(|constraint| constraint.field == "id")
        .and_then(|constraint| constraint.default_sequence)
        .expect("serial sequence default");
    let sequence_before = cassie
        .midge
        .get_sequence(&sequence_name)
        .expect("read sequence before denied writes")
        .expect("sequence metadata");

    // Act
    let default_cell = cassie.execute_sql(
        &writer,
        "INSERT INTO public.sequence_default_write_paths (id, body) VALUES (DEFAULT, 'denied')",
        Vec::new(),
    );
    let overriding_user = cassie.execute_sql(
        &writer,
        "INSERT INTO public.sequence_default_write_paths (id, body) OVERRIDING USER VALUE VALUES (10, 'denied')",
        Vec::new(),
    );
    let update_default = cassie.execute_sql(
        &writer,
        "UPDATE public.sequence_default_write_paths SET id = DEFAULT",
        Vec::new(),
    );
    let merge_default = cassie.execute_sql(
        &writer,
        "MERGE INTO public.sequence_default_write_paths AS target USING (VALUES (11, 'merge')) AS source(id, body) ON target.id = source.id WHEN NOT MATCHED THEN INSERT (body) VALUES (source.body)",
        Vec::new(),
    );
    let copy_omitted_default = cassie.execute_sql(
        &writer,
        "COPY public.sequence_default_write_paths (body) FROM STDIN",
        Vec::new(),
    );
    let rows = cassie
        .execute_sql(
            &root,
            "SELECT id FROM public.sequence_default_write_paths",
            Vec::new(),
        )
        .expect("inspect rows after denied writes");
    let sequence_after = cassie
        .midge
        .get_sequence(&sequence_name)
        .expect("read sequence after denied writes")
        .expect("sequence metadata");

    // Assert
    for result in [
        default_cell,
        overriding_user,
        update_default,
        merge_default,
        copy_omitted_default,
    ] {
        assert!(matches!(
            result,
            Err(CassieError::InsufficientPrivilege)
        ));
    }
    assert_eq!(rows.rows(), vec![vec![Value::Int64(9)]]);
    assert_eq!(sequence_after.current_value, sequence_before.current_value);
    assert_eq!(sequence_after.is_called, sequence_before.is_called);
}

#[test]
fn should_report_direct_acl_privilege_state() {
    // Arrange
    let (cassie, root, _reader, _projector) = role_grant_fixture();
    for role in ["reader", "projector"] {
        cassie
            .execute_sql(
                &root,
                &format!("GRANT USAGE ON SCHEMA public TO {role}"),
                Vec::new(),
            )
            .expect("grant schema usage");
    }
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant reader select");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE public.authorization_documents TO projector",
            Vec::new(),
        )
        .expect("grant projector access");

    // Act
    let privileges = cassie
        .execute_sql(
            &root,
            "SELECT has_database_privilege('reader', 'postgres', 'CONNECT'), \
                    has_schema_privilege('reader', 'public', 'USAGE'), \
                    has_table_privilege('reader', 'public.authorization_documents', 'SELECT'), \
                    has_table_privilege('reader', 'public.authorization_documents', 'INSERT'), \
                    has_table_privilege('projector', 'public.authorization_documents', 'DELETE')",
            Vec::new(),
        )
        .expect("selected privilege functions");
    let acl = cassie
        .execute_sql(
            &root,
            "SELECT namespace.nspacl, relation.relacl \
             FROM pg_catalog.pg_namespace AS namespace \
             CROSS JOIN pg_catalog.pg_class AS relation \
             WHERE namespace.nspname = 'public' \
               AND relation.relname = 'authorization_documents'",
            Vec::new(),
        )
        .expect("selected ACL catalog values");

    // Assert
    assert_eq!(
        privileges.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
    assert_eq!(
        acl.rows(),
        vec![vec![
            Value::String("{projector=U/root,reader=U/root}".to_owned()),
            Value::String("{projector=arwd/root,reader=r/root}".to_owned()),
        ]]
    );
}

#[test]
fn should_evaluate_selected_privileges_for_current_and_named_roles() {
    // Arrange
    let (cassie, root, reader, _projector) = role_grant_fixture();

    // Act
    let initial = cassie
        .execute_sql(
            &reader,
            "SELECT has_database_privilege('postgres', 'CONNECT'), \
                    has_schema_privilege('public', 'USAGE'), \
                    has_table_privilege('public.authorization_documents', 'SELECT'), \
                    has_table_privilege('public.authorization_documents', 'INSERT')",
            Vec::new(),
        )
        .expect("current reader privileges before object grants");
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO reader", Vec::new())
        .expect("grant reader schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant reader table select");
    let current_role = cassie
        .execute_sql(
            &reader,
            "SELECT has_schema_privilege('public', 'USAGE'), \
                    has_table_privilege('public.authorization_documents', 'INSERT'), \
                    has_table_privilege('public.authorization_documents', 'insert, SELECT')",
            Vec::new(),
        )
        .expect("current reader privileges after grants");
    let named_roles = cassie
        .execute_sql(
            &root,
            "SELECT has_table_privilege('reader', 'public.authorization_documents', 'select'), \
                    has_table_privilege('reader', 'public.authorization_documents', 'INSERT'), \
                    has_table_privilege('root', 'public.authorization_documents', 'DELETE')",
            Vec::new(),
        )
        .expect("explicit persisted-role privileges");
    let invalid_privilege = cassie.execute_sql(
        &root,
        "SELECT has_table_privilege('reader', 'public.authorization_documents', 'TRUNCATE')",
        Vec::new(),
    );
    let missing_role = cassie.execute_sql(
        &root,
        "SELECT has_table_privilege('missing_role', 'public.authorization_documents', 'SELECT')",
        Vec::new(),
    );
    let trusted_embedded = cassie.create_session("embedded_actor", Some("postgres".to_owned()));
    let embedded_privilege = cassie
        .execute_sql(
            &trusted_embedded,
            "SELECT has_table_privilege('public.authorization_documents', 'SELECT')",
            Vec::new(),
        )
        .expect("trusted embedded administrator context");

    // Assert
    assert_eq!(
        initial.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
        ]]
    );
    assert_eq!(
        current_role.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
    assert_eq!(
        named_roles.rows(),
        vec![vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
    assert!(matches!(invalid_privilege, Err(CassieError::Unsupported(_))));
    assert!(matches!(
        missing_role,
        Err(CassieError::CatalogObjectNotFound {
            kind: CatalogObjectKind::Role,
            ..
        })
    ));
    assert_eq!(embedded_privilege.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_reject_role_and_relation_oid_arguments_for_selected_privilege_helpers() {
    // Arrange
    let (cassie, root, _reader, _projector) = role_grant_fixture();
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.authorization_documents TO reader",
            Vec::new(),
        )
        .expect("grant reader select");
    let relation = cassie
        .execute_sql(
            &root,
            "SELECT oid FROM pg_catalog.pg_class WHERE relname = 'authorization_documents'",
            Vec::new(),
        )
        .expect("read relation oid");
    let Value::Int64(relation_oid) = &relation.rows()[0][0] else {
        panic!("expected relation OID");
    };

    // Act
    let rejected = cassie.execute_sql(
        &root,
        &format!("SELECT has_table_privilege('reader', '{relation_oid}', 'SELECT')"),
        Vec::new(),
    );
    let rejected_role_oid = cassie.execute_sql(
        &root,
        "SELECT has_table_privilege('10', 'public.authorization_documents', 'SELECT')",
        Vec::new(),
    );

    // Assert
    assert!(matches!(rejected, Err(CassieError::Unsupported(_))));
    assert!(matches!(
        rejected_role_oid,
        Err(CassieError::Unsupported(_))
    ));
}

#[test]
fn should_return_null_for_catalog_acl_without_direct_grants() {
    // Arrange
    let (cassie, root, _reader, _projector) = role_grant_fixture();

    // Act
    let acl = cassie
        .execute_sql(
            &root,
            "SELECT namespace.nspacl, relation.relacl \
             FROM pg_catalog.pg_namespace AS namespace \
             CROSS JOIN pg_catalog.pg_class AS relation \
             WHERE namespace.nspname = 'public' \
               AND relation.relname = 'authorization_documents'",
            Vec::new(),
        )
        .expect("read ungranted ACL catalog values");

    // Assert
    assert_eq!(acl.rows(), vec![vec![Value::Null, Value::Null]]);
}

#[test]
fn should_require_select_only_for_mutation_paths_that_read_target_values() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    let writer = create_table_and_role(&cassie, &root, "writer");
    cassie
        .execute_sql(&root, "GRANT USAGE ON SCHEMA public TO writer", Vec::new())
        .expect("grant schema usage");
    cassie
        .execute_sql(
            &root,
            "GRANT INSERT, UPDATE, DELETE ON TABLE public.authorization_documents TO writer",
            Vec::new(),
        )
        .expect("grant write privileges");

    // Act
    let update_all = cassie.execute_sql(
        &writer,
        "UPDATE public.authorization_documents SET body = 'writer-update'",
        Vec::new(),
    );
    let update_constant_subquery = cassie.execute_sql(
        &writer,
        "UPDATE public.authorization_documents SET body = (SELECT 'subquery-update')",
        Vec::new(),
    );
    let update_correlated_subquery = cassie.execute_sql(
        &writer,
        "UPDATE public.authorization_documents AS target SET body = (SELECT target.body)",
        Vec::new(),
    );
    let update_filtered = cassie.execute_sql(
        &writer,
        "UPDATE public.authorization_documents SET body = 'filtered-update' WHERE id = 1",
        Vec::new(),
    );
    let delete_filtered = cassie.execute_sql(
        &writer,
        "DELETE FROM public.authorization_documents WHERE id = 1",
        Vec::new(),
    );
    let insert_conflict_target = cassie.execute_sql(
        &writer,
        "INSERT INTO public.authorization_documents VALUES (1, 'duplicate') ON CONFLICT (id) DO NOTHING",
        Vec::new(),
    );
    let insert_returning = cassie.execute_sql(
        &writer,
        "INSERT INTO public.authorization_documents VALUES (2, 'returning') RETURNING id",
        Vec::new(),
    );
    let insert_without_row_read = cassie.execute_sql(
        &writer,
        "INSERT INTO public.authorization_documents VALUES (2, 'inserted')",
        Vec::new(),
    );
    let targetless_conflict = cassie.execute_sql(
        &writer,
        "INSERT INTO public.authorization_documents VALUES (3, 'targetless') ON CONFLICT DO NOTHING",
        Vec::new(),
    );
    let delete_all = cassie.execute_sql(
        &writer,
        "DELETE FROM public.authorization_documents",
        Vec::new(),
    );
    let remaining_rows = cassie
        .execute_sql(
            &root,
            "SELECT id FROM public.authorization_documents",
            Vec::new(),
        )
        .expect("inspect rows after writer operations");

    // Assert
    assert!(update_all.is_ok());
    assert!(update_constant_subquery.is_ok());
    assert!(
        matches!(
            update_correlated_subquery,
            Err(CassieError::InsufficientPrivilege)
        ),
        "expected a privilege error, got {update_correlated_subquery:?}"
    );
    assert!(matches!(
        update_filtered,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert!(matches!(
        delete_filtered,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert!(matches!(
        insert_conflict_target,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert!(matches!(
        insert_returning,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert!(insert_without_row_read.is_ok());
    assert!(targetless_conflict.is_ok());
    assert!(delete_all.is_ok());
    assert_eq!(remaining_rows.rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_reject_sequence_helpers_for_non_admin_roles() {
    // Arrange
    let cassie = cassie();
    let root = root_session(&cassie);
    let reader = create_table_and_role(&cassie, &root, "reader");
    cassie
        .execute_sql(
            &root,
            "CREATE SEQUENCE public.role_access_sequence",
            Vec::new(),
        )
        .expect("create sequence fixture");
    let root_next_value = cassie
        .execute_sql(
            &root,
            "SELECT nextval('public.role_access_sequence')",
            Vec::new(),
        )
        .expect("root retains sequence access");

    // Act
    let statements = [
        "SELECT nextval('public.role_access_sequence')",
        "SELECT pg_catalog.nextval('public.role_access_sequence')",
        "SELECT currval('public.role_access_sequence')",
        "SELECT setval('public.role_access_sequence', 7)",
        "SELECT lastval()",
        "SELECT pg_get_serial_sequence('public.authorization_documents', 'id')",
    ];
    let denied = statements.map(|statement| cassie.execute_sql(&reader, statement, Vec::new()));
    let root_next_value_after_denials = cassie
        .execute_sql(
            &root,
            "SELECT nextval('public.role_access_sequence')",
            Vec::new(),
        )
        .expect("denied sequence helpers do not mutate root sequence state");

    // Assert
    assert_eq!(root_next_value.rows(), vec![vec![Value::Int64(1)]]);
    assert_eq!(
        root_next_value_after_denials.rows(),
        vec![vec![Value::Int64(2)]]
    );
    for (statement, result) in statements.into_iter().zip(denied) {
        assert!(
            matches!(result, Err(CassieError::InsufficientPrivilege)),
            "expected 42501 for {statement}, got {result:?}"
        );
    }
}

#[path = "object_privilege_grants/lifecycle.rs"]
mod lifecycle;
#[path = "object_privilege_grants/identifier_visibility.rs"]
mod identifier_visibility;
