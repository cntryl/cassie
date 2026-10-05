//! Finite `pg_type` discovery under the selected #752 contract.

use super::support_sql_fixture::sql_fixture;
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
fn should_discover_every_supported_scalar_array_family() {
    // Arrange
    let fixture = sql_fixture("finite-type-array-families", &[]);
    fixture.cassie.startup().expect("startup");
    // Literal oracle: Cassie's selected private ARRAY OIDs, not PostgreSQL's
    // standard array OIDs or values obtained from the production DataType map.
    let expected = [
        (34_016, "boolean[]", 16),
        (34_017, "bytea[]", 17),
        (34_020, "bigint[]", 20),
        (34_021, "smallint[]", 21),
        (34_023, "int[]", 23),
        (34_025, "text[]", 25),
        (34_114, "json[]", 114),
        (34_701, "float[]", 701),
        (35_042, "char[]", 1_042),
        (35_043, "varchar[]", 1_043),
        (35_082, "date[]", 1_082),
        (35_083, "time[]", 1_083),
        (35_114, "timestamp[]", 1_114),
        (36_950, "uuid[]", 2_950),
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
    assert_eq!(selected.rows, expected);
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
        (16, "boolean", 1),
        (17, "bytea", -1),
        (20, "bigint", 8),
        (21, "smallint", 2),
        (23, "int", 4),
        (25, "text", -1),
        (114, "json", -1),
        (701, "float", 8),
        (705, "null", -2),
        (1_042, "char", -1),
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
    assert_eq!(selected.rows, expected);
    assert_eq!(
        attributes.rows,
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
    assert_eq!(selected.rows, Vec::<Vec<Value>>::new());
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
        vectors.rows,
        vec![
            type_row(100_002, "vector(2)", -1, 0),
            type_row(100_007, "vector(7)", -1, 0),
            type_row(100_011, "vector(11)", -1, 0),
        ]
    );
    let mut oids = std::collections::BTreeSet::new();
    for row in &all_types.rows {
        let Value::Int64(oid) = &row[0] else {
            panic!("registry OID is not an integer");
        };
        assert!(oids.insert(*oid), "duplicate registry OID");
    }
    // ARRAY(VECTOR(7016)) has the existing durable OID34016. Its excluded
    // wire identity must not replace or duplicate supported BOOLEAN[].
    assert_eq!(
        all_types
            .rows
            .iter()
            .find(|row| row[0] == Value::Int64(34_016)),
        Some(&type_row(34_016, "boolean[]", -1, 16))
    );
    assert_eq!(all_types.rows.len(), 32);
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
    assert_eq!(current.rows, vec![type_row(100_002, "vector(2)", -1, 0)]);
    assert_eq!(other.rows, vec![type_row(100_007, "vector(7)", -1, 0)]);
}
