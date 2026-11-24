//! LIKE composite primary-key clones retain ordered, target-owned constraints.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE TABLE clone_source(a INT, b INT, label TEXT, CONSTRAINT source_key PRIMARY KEY(b, a))",
    "INSERT INTO clone_source VALUES(1, 2, 'source')",
];

#[test]
fn should_clone_composite_primary_key_with_target_owned_identity() {
    // Arrange
    let fixture = sql_fixture("like-composite-primary-key", SETUP);
    // Act
    fixture
        .execute("CREATE TABLE clone_target(LIKE clone_source INCLUDING INDEXES)")
        .unwrap();
    // Assert
    let keys = fixture.cassie.catalog.get_constraints("clone_target");
    let mut members = keys
        .iter()
        .filter(|constraint| constraint.primary_key)
        .collect::<Vec<_>>();
    members.sort_by_key(|constraint| constraint.primary_key_ordinal);
    assert_eq!(
        members
            .iter()
            .map(|constraint| constraint.field.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a"]
    );
    assert!(members
        .iter()
        .all(|constraint| constraint.primary_key_name.as_deref() == Some("clone_target_pkey")));
    let indexes = fixture.cassie.catalog.list_indexes("clone_target");
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].fields, vec!["b", "a"]);
    fixture
        .execute("INSERT INTO clone_target VALUES(1, 2, 'target'), (1, 3, 'second')")
        .unwrap();
    assert!(fixture
        .execute("INSERT INTO clone_target VALUES(1, 2, 'duplicate')")
        .is_err());
    assert!(fixture
        .execute("INSERT INTO clone_target VALUES(NULL, 2, 'null')")
        .is_err());
    assert!(fixture
        .execute("INSERT INTO clone_target VALUES(1, NULL, 'null')")
        .is_err());
    fixture.execute("DROP TABLE clone_source").unwrap();
    assert_eq!(
        fixture.rows("SELECT a, b FROM clone_target ORDER BY b"),
        vec![
            vec![Value::Int64(1), Value::Int64(2)],
            vec![Value::Int64(1), Value::Int64(3)]
        ]
    );
}

#[test]
fn should_discard_composite_primary_key_clone_at_savepoint_rollback() {
    // Arrange
    let fixture = sql_fixture("like-composite-primary-key-rollback", SETUP);
    fixture.execute("BEGIN").unwrap();
    fixture.execute("SAVEPOINT before_clone").unwrap();
    // Act
    fixture
        .execute("CREATE TABLE clone_target(LIKE clone_source INCLUDING ALL)")
        .unwrap();
    fixture
        .execute("INSERT INTO clone_target VALUES(1, 2, 'target')")
        .unwrap();
    fixture.execute("ROLLBACK TO before_clone").unwrap();
    fixture.execute("COMMIT").unwrap();
    // Assert
    assert!(!fixture.cassie.catalog.exists("clone_target"));
    assert_eq!(fixture.cassie.catalog.list_indexes("clone_target").len(), 0);
    fixture
        .execute("CREATE TABLE clone_target(LIKE clone_source INCLUDING ALL)")
        .unwrap();
    fixture
        .execute("INSERT INTO clone_target VALUES(1, 2, 'target')")
        .unwrap();
}

#[test]
fn should_keep_not_null_when_clone_excludes_composite_index() {
    // Arrange
    let fixture = sql_fixture("like-primary-key-excluded", SETUP);
    // Act
    fixture
        .execute("CREATE TABLE clone_target(LIKE clone_source INCLUDING ALL EXCLUDING INDEXES)")
        .unwrap();
    // Assert
    assert_eq!(fixture.cassie.catalog.list_indexes("clone_target").len(), 0);
    fixture
        .execute("INSERT INTO clone_target VALUES(1, 2, 'first'), (1, 2, 'second')")
        .unwrap();
    assert!(fixture
        .execute("INSERT INTO clone_target VALUES(NULL, 2, 'null')")
        .is_err());
}

#[test]
fn should_recover_cloned_composite_primary_key_after_local_restart() {
    // Arrange
    let directory =
        crate::support_catalog_directory::CatalogDirectory::new("like-primary-key-restart");
    {
        let cassie = cassie::app::Cassie::new_with_data_dir_and_config(
            &directory.0,
            cassie::config::CassieRuntimeConfig::default(),
        )
        .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        for sql in SETUP {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        // Act
        for sql in [
            "CREATE TABLE clone_target(LIKE clone_source INCLUDING ALL)",
            "INSERT INTO clone_target VALUES(1, 2, 'target')",
            "DROP TABLE clone_source",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
    }
    let cassie = cassie::app::Cassie::new_with_data_dir_and_config(
        &directory.0,
        cassie::config::CassieRuntimeConfig::default(),
    )
    .unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    // Assert
    assert!(cassie
        .execute_sql(
            &session,
            "INSERT INTO clone_target VALUES(1, 2, 'duplicate')",
            vec![]
        )
        .is_err());
    cassie
        .execute_sql(
            &session,
            "INSERT INTO clone_target VALUES(1, 3, 'second')",
            vec![],
        )
        .unwrap();
    let indexes = cassie.catalog.list_indexes("clone_target");
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].fields, vec!["b", "a"]);
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE clone_target DROP CONSTRAINT clone_target_pkey",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            &session,
            "INSERT INTO clone_target VALUES(1, 2, 'duplicate')",
            vec![],
        )
        .unwrap();
}

#[test]
fn should_avoid_existing_relation_names_for_cloned_primary_key() {
    // Arrange
    let fixture = sql_fixture("like-primary-key-name-collision", &[
        "CREATE TABLE clone_source(a INT, b INT, CONSTRAINT clone_target_pkey PRIMARY KEY(b, a))",
    ]);
    // Act
    fixture
        .execute("CREATE TABLE clone_target(LIKE clone_source INCLUDING INDEXES)")
        .unwrap();
    // Assert
    let indexes = fixture.cassie.catalog.list_indexes("clone_target");
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].name, "clone_target_pkey1");
    fixture.execute("DROP TABLE clone_source").unwrap();
    fixture
        .execute("INSERT INTO clone_target VALUES(1, 2)")
        .unwrap();
}

#[test]
fn should_ignore_other_schema_names_when_naming_cloned_primary_key() {
    // Arrange
    let fixture = sql_fixture(
        "like-primary-key-other-schema",
        &[
            "CREATE SCHEMA other_scope",
            "CREATE TABLE other_scope.occupied(value INT)",
            "CREATE INDEX clone_target_pkey ON other_scope.occupied(value)",
            "CREATE TABLE clone_source(a INT, b INT, CONSTRAINT source_key PRIMARY KEY(b, a))",
        ],
    );
    // Act
    fixture
        .execute("CREATE TABLE clone_target(LIKE clone_source INCLUDING INDEXES)")
        .unwrap();
    // Assert
    let indexes = fixture.cassie.catalog.list_indexes("clone_target");
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].name, "clone_target_pkey");
}

#[test]
fn should_skip_occupied_relation_names_when_naming_cloned_primary_key() {
    // Arrange
    let fixture = sql_fixture(
        "like-primary-key-relation-collision",
        &[
            "CREATE TABLE clone_source(a INT, b INT, CONSTRAINT source_key PRIMARY KEY(b, a))",
            "CREATE VIEW clone_target_pkey AS SELECT a FROM clone_source",
            "CREATE TABLE clone_target_pkey1(value INT)",
        ],
    );
    // Act
    fixture
        .execute("CREATE TABLE clone_target(LIKE clone_source INCLUDING INDEXES)")
        .unwrap();
    // Assert
    let indexes = fixture.cassie.catalog.list_indexes("clone_target");
    assert_eq!(indexes.len(), 1);
    assert_eq!(indexes[0].name, "clone_target_pkey2");
    fixture.execute("REINDEX INDEX clone_target_pkey2").unwrap();
}

#[test]
fn should_address_cloned_primary_key_from_quoted_target_name() {
    // Arrange
    let fixture = sql_fixture("like-primary-key-quoted-target", SETUP);
    fixture
        .execute("CREATE TABLE \"clone.target\"(LIKE clone_source INCLUDING INDEXES)")
        .unwrap();
    // Act
    fixture
        .execute("REINDEX INDEX \"clone.target_pkey\"")
        .unwrap();
    fixture
        .execute("ALTER TABLE \"clone.target\" DROP CONSTRAINT \"clone.target_pkey\"")
        .unwrap();
    // Assert
    assert_eq!(
        fixture
            .cassie
            .catalog
            .list_indexes("\"clone.target\"")
            .len(),
        0
    );
    fixture
        .execute("INSERT INTO \"clone.target\" VALUES(1, 2, 'one'), (1, 2, 'two')")
        .unwrap();
}
