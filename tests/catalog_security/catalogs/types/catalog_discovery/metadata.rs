//! Finite `pg_type` discovery under the selected #752 contract.

use super::support_sql_fixture::sql_fixture;
use cassie::app::Cassie;
use cassie::types::Value;

#[path = "../identifier/metadata.rs"]
mod identifier_residuals;
#[path = "../oid_identity/metadata.rs"]
mod oid_identity_wave;
#[path = "../text_search_identifier_lookup/restart.rs"]
mod text_search_identifier_lookup_wave;
#[path = "../vector_catalog.rs"]
mod vector_catalog_contracts;

fn type_row(oid: i64, name: &str, length: i64, element: i64) -> Vec<Value> {
    vec![
        Value::Int64(oid),
        Value::String(name.to_string()),
        Value::Int64(length),
        Value::Int64(element),
    ]
}

struct TypeCatalogDirectory(String);

impl Drop for TypeCatalogDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_preserve_regclass_identity_across_catalog_lifecycle() {
    // Arrange
    super::support_sql::use_local_storage();
    let path = super::support_sql::data_dir("issue-818-regclass-lifecycle-restart");
    let _directory = TypeCatalogDirectory(path.clone());
    let object_oid = {
        let cassie = Cassie::new_with_data_dir(&path).expect("first Cassie instance");
        cassie.startup().expect("first startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA issue_818_lifecycle", vec![])
            .expect("create relation schema");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_818_lifecycle.before_name (id INTEGER)",
                vec![],
            )
            .expect("create relation");
        let initial = cassie
            .execute_sql(
                &session,
                "SELECT 'issue_818_lifecycle.before_name'::regclass::oid",
                vec![],
            )
            .expect("read original relation OID");
        let Value::Int64(object_oid) = &initial.rows()[0][0] else {
            panic!("regclass OID is not an integer");
        };
        let object_oid = *object_oid;

        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin rename rollback");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE issue_818_lifecycle.before_name RENAME TO rolled_back_name",
                vec![],
            )
            .expect("rename relation inside transaction");
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .expect("roll back relation rename");
        let rolled_back = cassie
            .execute_sql(
                &session,
                "SELECT 'issue_818_lifecycle.before_name'::regclass::oid, \
                        to_regclass('issue_818_lifecycle.rolled_back_name') IS NULL",
                vec![],
            )
            .expect("resolve relation after rollback");
        assert_eq!(
            rolled_back.rows(),
            vec![vec![Value::Int64(object_oid), Value::Bool(true)]]
        );
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE issue_818_lifecycle.before_name RENAME TO persisted_name",
                vec![],
            )
            .expect("rename relation for restart check");
        object_oid
    };

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopened Cassie instance");
    cassie.startup().expect("restart startup");
    let session = cassie.create_session("tester", None);
    let after_restart = cassie
        .execute_sql(
            &session,
            "SELECT to_regclass('issue_818_lifecycle.persisted_name')::oid, \
                    to_regclass('issue_818_lifecycle.before_name') IS NULL",
            vec![],
        )
        .expect("resolve renamed relation after restart");
    cassie
        .execute_sql(
            &session,
            "DROP TABLE issue_818_lifecycle.persisted_name",
            vec![],
        )
        .expect("drop renamed relation");
    let after_drop = cassie
        .execute_sql(
            &session,
            "SELECT to_regclass('issue_818_lifecycle.persisted_name') IS NULL",
            vec![],
        )
        .expect("resolve dropped relation name");

    // Assert
    assert_eq!(
        after_restart.rows(),
        vec![vec![Value::Int64(object_oid), Value::Bool(true)]]
    );
    assert_eq!(after_drop.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_preserve_quoted_regclass_identity_through_oid_lookup() {
    // Arrange
    let fixture = sql_fixture(
        "issue-818-quoted-regclass-oid-identity",
        &[
            "CREATE SCHEMA \"Issue_818\"",
            "CREATE TABLE \"Issue_818\".\"QuotedTable\" (id INTEGER)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT CAST(CAST('\"Issue_818\".\"QuotedTable\"' AS regclass) AS text), \
                    CAST(CAST('\"Issue_818\".\"QuotedTable\"' AS regclass) AS oid) = \
                      CAST(to_regclass('\"Issue_818\".\"QuotedTable\"') AS oid)",
        )
        .expect("resolve the quoted relation's regclass identity");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("\"Issue_818\".\"QuotedTable\"".to_owned()),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_match_regnamespace_names_by_oid_identity() {
    // Arrange
    let fixture = sql_fixture("issue-818-regnamespace-lookup", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT 'public'::regnamespace = to_regnamespace('public')")
        .expect("regnamespace identity and nullable lookup");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_accept_the_postgresql_oid_scalar_type() {
    // Arrange
    let fixture = sql_fixture("issue-818-oid-scalar-type", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture.execute("SELECT 23::oid").expect("OID scalar type");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(23)]]);
}

#[test]
fn should_preserve_oid_integer_identity() {
    // Arrange
    let fixture = sql_fixture("issue-818-oid-integer-identity", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT CAST(CAST(42 AS oid) AS bigint), \
                    CAST(42 AS oid) = CAST(42 AS integer)",
        )
        .expect("cast and compare an OID with integer values");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(42), Value::Bool(true)]]
    );
}

#[test]
fn should_advertise_pgcrypto_in_available_extension_catalog() {
    // Arrange
    let fixture = sql_fixture("issue-843-pgcrypto-catalog-identity", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT name, default_version FROM pg_catalog.pg_available_extensions WHERE name = 'pgcrypto'",
        )
        .expect("available pgcrypto extension catalog row");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("pgcrypto".to_owned()),
            Value::String("1.4".to_owned()),
        ]]
    );
}

include!("nulls.rs");

include!("restart.rs");

include!("semantics.rs");
