//! Finite `pg_type` discovery under the selected #752 contract.

use super::support_sql_fixture::sql_fixture;
use cassie::app::Cassie;
use cassie::types::Value;

fn type_row(oid: i64, name: &str, length: i64, element: i64) -> Vec<Value> {
    vec![
        Value::Int64(oid),
        Value::String(name.to_string()),
        Value::Int64(length),
        Value::Int64(element),
    ]
}

#[test]
fn should_discover_scalar_array_families_with_postgresql_identities() {
    // Arrange
    let fixture = sql_fixture("finite-type-array-families", &[]);
    fixture.cassie.startup().expect("startup");
    // Literal oracle from PostgreSQL 18.6 pg_type, independent of Cassie's map.
    let expected = [
        (199, "_json", 114),
        (1_000, "_bool", 16),
        (1_001, "_bytea", 17),
        (1_005, "_int2", 21),
        (1_007, "_int4", 23),
        (1_009, "_text", 25),
        (1_014, "_bpchar", 1_042),
        (1_015, "_varchar", 1_043),
        (1_016, "_int8", 20),
        (1_022, "_float8", 701),
        (1_115, "_timestamp", 1_114),
        (1_182, "_date", 1_082),
        (1_183, "_time", 1_083),
        (2_951, "_uuid", 2_950),
    ]
    .map(|(oid, name, element)| type_row(oid, name, -1, element))
    .to_vec();

    // Act
    let selected = fixture
        .execute(
            "SELECT oid, typname, typlen, typelem FROM pg_catalog.pg_type WHERE typelem <> 0 ORDER BY oid",
        )
        .expect("finite array registry");

    // Assert
    assert_eq!(selected.rows(), expected);
}

#[test]
fn should_preserve_scalar_registry_identity_across_column_modifiers() {
    // Arrange
    let fixture = sql_fixture("finite-type-scalar-families", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE TABLE p04_character_modifiers (c3 CHAR(3), c9 CHAR(9), v8 VARCHAR(8), v12 VARCHAR(12))",
        )
        .expect("character columns with distinct modifiers");
    let expected = [
        (16, "bool", 1),
        (17, "bytea", -1),
        (20, "int8", 8),
        (21, "int2", 2),
        (23, "int4", 4),
        (25, "text", -1),
        (114, "json", -1),
        (701, "float8", 8),
        (705, "unknown", -2),
        (1_042, "bpchar", -1),
        (1_043, "varchar", -1),
        (1_082, "date", 4),
        (1_083, "time", 8),
        (1_114, "timestamp", 8),
        (2_950, "uuid", 16),
    ]
    .map(|(oid, name, length)| type_row(oid, name, length, 0))
    .to_vec();

    // Act
    let selected = fixture
        .execute(
            "SELECT oid, typname, typlen, typelem FROM pg_catalog.pg_type WHERE typelem = 0 AND oid < 100000 ORDER BY oid",
        )
        .expect("finite scalar registry");
    let attributes = fixture
        .execute(
            "SELECT attname, atttypid, atttypmod FROM pg_catalog.pg_attribute WHERE attrelid = 'p04_character_modifiers' AND attname IN ('c3', 'c9', 'v8', 'v12') ORDER BY attnum",
        )
        .expect("character column modifiers");

    // Assert
    assert_eq!(selected.rows(), expected);
    assert_eq!(
        attributes.rows(),
        vec![
            vec![
                Value::String("c3".to_string()),
                Value::Int64(1_042),
                Value::Int64(7)
            ],
            vec![
                Value::String("c9".to_string()),
                Value::Int64(1_042),
                Value::Int64(13)
            ],
            vec![
                Value::String("v8".to_string()),
                Value::Int64(1_043),
                Value::Int64(12)
            ],
            vec![
                Value::String("v12".to_string()),
                Value::Int64(1_043),
                Value::Int64(16)
            ],
        ]
    );
}

#[test]
fn should_omit_unreferenced_vector_type_rows() {
    // Arrange
    let fixture = sql_fixture("finite-type-no-vector-schema", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT oid, typname FROM pg_catalog.pg_type WHERE typcategory = 'V'")
        .expect("unreferenced vector registry");

    // Assert
    assert_eq!(selected.rows(), Vec::<Vec<Value>>::new());
}

#[test]
fn should_resolve_regtype_aliases_with_nullable_lookup() {
    // Arrange
    let fixture = sql_fixture("regtype-alias-and-missing-name", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT 'pg_catalog.int4'::regtype, 'integer'::regtype = 'int4'::regtype, to_regtype('type_that_does_not_exist') IS NULL",
        )
        .expect("regtype alias and nullable lookup");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(23), Value::Bool(true), Value::Bool(true),]]
    );
}

struct TypeCatalogDirectory(String);

impl Drop for TypeCatalogDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn should_preserve_declared_type_identity_with_typmod_after_restart() {
    // Arrange
    super::support_sql::use_local_storage();
    let path = super::support_sql::data_dir("issue-904-type-catalog-restart");
    let _directory = TypeCatalogDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("first Cassie instance");
        cassie.startup().expect("first startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_904_persisted_type (amount NUMERIC(5, 2))",
                vec![],
            )
            .expect("persisted numeric column");
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopened Cassie instance");
    cassie.startup().expect("restart startup");
    let session = cassie.create_session("tester", None);
    let selected = cassie
        .execute_sql(
            &session,
            "SELECT atttypid, atttypmod FROM pg_attribute WHERE attrelid = 'issue_904_persisted_type' AND attname = 'amount'",
            vec![],
        )
        .expect("persisted type descriptor");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(1_700), Value::Int64(327_686)]]
    );
}

#[test]
fn should_deduplicate_supported_schema_vector_identities() {
    // Arrange
    let fixture = sql_fixture("finite-type-vector-identities", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE TABLE p04_vector_documents (v2 VECTOR(2), v7_first VECTOR(7), v7_second VECTOR(7), storage_only VECTOR(32768), excluded_array VECTOR(7016)[])",
        )
        .expect("vector schemas, including existing unsupported wire identities");
    fixture
        .execute(
            "CREATE VIEW p04_vector_view AS SELECT CAST(NULL AS VECTOR(11)) AS embedding FROM p04_vector_documents",
        )
        .expect("view-only scalar vector dimension");

    // Act
    let vectors = fixture
        .execute(
            "SELECT oid, typname, typlen, typelem FROM pg_catalog.pg_type WHERE typcategory = 'V' ORDER BY oid",
        )
        .expect("schema-referenced vector registry");
    let all_types = fixture
        .execute("SELECT oid, typname, typlen, typelem FROM pg_catalog.pg_type ORDER BY oid")
        .expect("finite unique registry");

    // Assert
    assert_eq!(
        vectors.rows(),
        vec![
            type_row(100_002, "vector(2)", -1, 0),
            type_row(100_007, "vector(7)", -1, 0),
            type_row(100_011, "vector(11)", -1, 0),
        ]
    );
    let mut oids = std::collections::BTreeSet::new();
    for row in all_types.rows() {
        let Value::Int64(oid) = &row[0] else {
            panic!("registry OID is not an integer");
        };
        assert!(oids.insert(*oid), "duplicate registry OID");
    }
    // ARRAY(VECTOR(7016)) is outside the selected wire type inventory and must
    // not replace the PostgreSQL standard BOOLEAN[] identity.
    assert_eq!(
        all_types
            .rows()
            .iter()
            .find(|row| row[0] == Value::Int64(1_000)),
        Some(&type_row(1_000, "_bool", -1, 16))
    );
    assert_eq!(all_types.rows().len(), 32);
}

#[test]
fn should_discover_vector_types_within_the_current_database() {
    // Arrange
    let fixture = sql_fixture("finite-type-vector-database-scope", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE TABLE p04_local_vectors (embedding VECTOR(2))")
        .expect("current database vector schema");
    fixture
        .execute("CREATE DATABASE p04_vector_tenant")
        .expect("second database");
    let tenant = fixture
        .cassie
        .create_session("tester", Some("p04_vector_tenant".to_string()));
    fixture
        .cassie
        .execute_sql(
            &tenant,
            "CREATE TABLE p04_local_vectors (embedding VECTOR(7))",
            vec![],
        )
        .expect("second database vector schema");

    // Act
    let current = fixture
        .execute(
            "SELECT oid, typname, typlen, typelem FROM pg_catalog.pg_type WHERE typcategory = 'V' ORDER BY oid",
        )
        .expect("current database vector types");
    let other = fixture
        .cassie
        .execute_sql(
            &tenant,
            "SELECT oid, typname, typlen, typelem FROM pg_catalog.pg_type WHERE typcategory = 'V' ORDER BY oid",
            vec![],
        )
        .expect("second database vector types");

    // Assert
    assert_eq!(current.rows(), vec![type_row(100_002, "vector(2)", -1, 0)]);
    assert_eq!(other.rows(), vec![type_row(100_007, "vector(7)", -1, 0)]);
}

#[test]
fn should_return_null_for_missing_regtype_aliases() {
    // Arrange
    let fixture = sql_fixture("regtype-alias-and-missing-name", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT 'pg_catalog.int4'::regtype::oid, 'integer'::regtype = 'int4'::regtype, to_regtype('type_that_does_not_exist') IS NULL",
        )
        .expect("regtype alias and nullable lookup");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(23), Value::Bool(true), Value::Bool(true),]]
    );
}

#[test]
fn should_resolve_regclass_names_across_search_path_then_drop() {
    // Arrange
    let fixture = sql_fixture("issue-818-regclass-search-path-drop", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA issue_818_regclass_lookup")
        .expect("lookup schema");
    fixture
        .execute("CREATE TABLE issue_818_regclass_lookup.item (id INTEGER)")
        .expect("lookup table");
    fixture
        .execute("SET search_path TO issue_818_regclass_lookup, public")
        .expect("search path");

    // Act
    let resolved = fixture
        .execute(
            "SELECT to_regclass('item') = to_regclass('issue_818_regclass_lookup.item'), to_regclass('missing_item') IS NULL",
        )
        .expect("regclass lookup in search path");
    fixture
        .execute("DROP TABLE item")
        .expect("drop lookup table");
    let missing = fixture
        .execute("SELECT to_regclass('item') IS NULL")
        .expect("missing regclass lookup after drop");

    // Assert
    assert_eq!(
        resolved.rows(),
        &[vec![Value::Bool(true), Value::Bool(true),]]
    );
    assert_eq!(missing.rows(), &[vec![Value::Bool(true)]]);
}

#[test]
fn should_discover_postgresql_catalog_rows_for_supported_type_families() {
    // Arrange
    let fixture = sql_fixture("issue-817-additional-builtin-type-rows", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT oid, typname, typlen FROM pg_catalog.pg_type WHERE typname IN ('xml', 'money', 'point', 'lseg', 'path', 'box', 'polygon', 'line', 'circle', 'int4range', 'int4multirange') ORDER BY oid",
        )
        .expect("additional finite PostgreSQL type rows");

    // Assert
    let expected = [
        (142, "xml", -1),
        (600, "point", 16),
        (601, "lseg", 32),
        (602, "path", -1),
        (603, "box", 32),
        (604, "polygon", -1),
        (628, "line", 24),
        (718, "circle", 24),
        (790, "money", 8),
        (3_904, "int4range", -1),
        (4_451, "int4multirange", -1),
    ]
    .map(|(oid, name, length)| {
        vec![
            Value::Int64(oid),
            Value::String(name.to_owned()),
            Value::Int64(length),
        ]
    })
    .to_vec();
    assert_eq!(selected.rows(), expected);
}

#[test]
fn should_resolve_regprocedure_signature_aliases_including_missing_overloads() {
    // Arrange
    let fixture = sql_fixture("issue-818-regprocedure-overload-resolution", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT to_regprocedure('pg_catalog.abs(integer)') = to_regprocedure('pg_catalog.abs(int4)'), to_regprocedure('pg_catalog.abs(text)') IS NULL",
        )
        .expect("regprocedure aliases and nullable lookup");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Bool(true), Value::Bool(true)]]
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
fn should_return_null_for_missing_regnamespace_lookup() {
    // Arrange
    let fixture = sql_fixture("issue-818-regnamespace-missing-lookup", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT to_regnamespace('missing_schema') IS NULL")
        .expect("missing regnamespace lookup");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_resolve_builtin_collation_through_to_regcollation() {
    // Arrange
    let fixture = sql_fixture("issue-818-to-regcollation-builtin", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT to_regcollation('pg_catalog.\"default\"') IS NOT NULL")
        .expect("built-in collation lookup");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_resolve_a_created_role_through_to_regrole() {
    // Arrange
    let fixture = sql_fixture(
        "issue-818-to-regrole-created-role",
        &["CREATE ROLE issue_818_lookup_role"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT to_regrole('issue_818_lookup_role') IS NOT NULL")
        .expect("created role lookup");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_resolve_a_builtin_function_through_to_regproc() {
    // Arrange
    let fixture = sql_fixture("issue-818-to-regproc-builtin-function", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT to_regproc('pg_catalog.now') IS NOT NULL")
        .expect("built-in regproc lookup");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_report_a_resolved_scalar_type_with_pg_typeof() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-typeof-resolution", &[]);
    fixture.cassie.startup().expect("startup");
    let expected = vec![vec![Value::String("integer".to_owned())]];

    // Act
    let selected = fixture
        .execute("SELECT pg_typeof(1)::text")
        .expect("resolved scalar type name");

    // Assert
    assert_eq!(selected.rows(), expected);
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
fn should_support_regproc_text_io() {
    // Arrange
    let fixture = sql_fixture("issue-818-regproc-text-io", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT 'pg_catalog.now'::regproc")
        .expect("regproc text input and output");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::String("now".to_owned())]]);
}

#[test]
fn should_support_regoper_text_io() {
    // Arrange
    let fixture = sql_fixture("issue-818-regoper-text-io", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT 'pg_catalog.|/'::regoper")
        .expect("regoper text input and output");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::String("|/".to_owned())]]);
}

#[test]
fn should_resolve_regoper_lookup_results() {
    // Arrange
    let fixture = sql_fixture("issue-818-to-regoper-lookup", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT to_regoper('pg_catalog.|/'), to_regoper('pg_catalog.no_such_operator') IS NULL",
        )
        .expect("regoper lookup and missing-object behavior");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("|/".to_owned()), Value::Bool(true)]]
    );
}

#[test]
fn should_support_regoperator_text_io() {
    // Arrange
    let fixture = sql_fixture("issue-818-regoperator-text-io", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT 'pg_catalog.+(integer,integer)'::regoperator")
        .expect("regoperator text input and output");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("+(integer,integer)".to_owned())]]
    );
}

#[test]
fn should_resolve_regoperator_overload_lookup_results() {
    // Arrange
    let fixture = sql_fixture("issue-818-to-regoperator-lookup", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT to_regoperator('pg_catalog.+(integer,integer)'), to_regoperator('pg_catalog.+(text,text)') IS NULL",
        )
        .expect("regoperator overload lookup and missing-object behavior");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("+(integer,integer)".to_owned()),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_support_regrole_text_io() {
    // Arrange
    let fixture = sql_fixture(
        "issue-818-regrole-text-io",
        &["CREATE ROLE issue_818_regrole_text"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT 'issue_818_regrole_text'::regrole")
        .expect("regrole text input and output");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("issue_818_regrole_text".to_owned())]]
    );
}

#[test]
fn should_support_regconfig_text_io() {
    // Arrange
    let fixture = sql_fixture("issue-818-regconfig-text-io", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT 'pg_catalog.simple'::regconfig")
        .expect("regconfig text input and output");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("simple".to_owned())]]
    );
}

#[test]
fn should_support_regdictionary_text_io() {
    // Arrange
    let fixture = sql_fixture("issue-818-regdictionary-text-io", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT 'pg_catalog.simple'::regdictionary")
        .expect("regdictionary text input and output");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("simple".to_owned())]]
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
