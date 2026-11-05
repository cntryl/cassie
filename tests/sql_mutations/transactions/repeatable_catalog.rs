//! The bounded old-plus-own catalog profile excludes stale DDL dependencies.

use cassie::app::CassieError;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_publish_own_catalog_changes_to_the_next_command() {
    // Arrange
    let fixture = sql_fixture("rr-own-catalog", &[]);
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");
    fixture.execute("SELECT 1").expect("initial view");
    fixture
        .execute("CREATE TABLE rr_own_catalog (id INT)")
        .expect("own table");
    fixture
        .execute("SAVEPOINT before_column")
        .expect("savepoint");

    // Act
    fixture
        .execute("ALTER TABLE rr_own_catalog ADD COLUMN payload TEXT")
        .expect("own column");
    let changed = fixture.execute("SELECT column_name FROM information_schema.columns WHERE table_name = 'rr_own_catalog' ORDER BY ordinal_position").expect("own catalog");
    fixture
        .execute("ROLLBACK TO SAVEPOINT before_column")
        .expect("restore own metadata");
    let restored = fixture.execute("SELECT column_name FROM information_schema.columns WHERE table_name = 'rr_own_catalog' ORDER BY ordinal_position").expect("restored catalog");
    fixture.execute("ROLLBACK").expect("retire");

    // Assert
    assert_eq!(
        changed.rows(),
        &[
            vec![Value::String("id".into())],
            vec![Value::String("payload".into())]
        ]
    );
    assert_eq!(restored.rows(), &[vec![Value::String("id".into())]]);
}

#[test]
fn should_exclude_the_ctas_target_from_its_command_source() {
    // Arrange
    let fixture = sql_fixture("rr-ctas-catalog", &[]);
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");

    // Act
    fixture.execute("CREATE TABLE rr_ctas_catalog AS SELECT count(*) AS prior_count FROM information_schema.tables WHERE table_name = 'rr_ctas_catalog'").expect("CTAS");
    let source = fixture
        .execute("SELECT prior_count FROM rr_ctas_catalog")
        .expect("source result");
    let following = fixture
        .execute(
            "SELECT count(*) FROM information_schema.tables WHERE table_name = 'rr_ctas_catalog'",
        )
        .expect("next command catalog");
    fixture.execute("ROLLBACK").expect("retire");

    // Assert
    assert_eq!(source.rows(), &[vec![Value::Int64(0)]]);
    assert_eq!(following.rows(), &[vec![Value::Int64(1)]]);
}

#[test]
fn should_reject_stale_catalog_dependencies_before_every_ddl_attempt() {
    // Arrange
    for ddl in [
        "CREATE TABLE IF NOT EXISTS rr_stale_target (id INT)",
        "ALTER TABLE rr_stale_target ADD COLUMN own_column INT",
        "CREATE INDEX rr_stale_index ON rr_stale_target (outside_column)",
    ] {
        let fixture = sql_fixture(
            "rr-stale-catalog",
            &["CREATE TABLE rr_stale_target (id INT)"],
        );
        fixture
            .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
            .expect("begin");
        fixture.execute("SELECT 1").expect("initial view");
        let writer = fixture.cassie.create_session("tester", None);
        fixture
            .cassie
            .execute_sql(
                &writer,
                "ALTER TABLE rr_stale_target ADD COLUMN outside_column INT",
                Vec::new(),
            )
            .expect("outside DDL");

        // Act
        let failure = fixture.execute(ddl);
        fixture.execute("ROLLBACK").expect("retire failed block");

        // Assert
        assert!(
            matches!(
                failure,
                Err(CassieError::Transaction {
                    kind: cassie::app::TransactionErrorKind::SerializationFailure,
                    ..
                })
            ),
            "{ddl}: {failure:?}"
        );
    }
}

#[test]
fn should_keep_current_descriptors_separate_from_old_catalog_rows() {
    // Arrange
    let fixture = sql_fixture("rr-current-descriptors", &[]);
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");
    fixture
        .execute("CREATE TABLE rr_own_descriptor (id INT)")
        .expect("own DDL");
    let writer = fixture.cassie.create_session("tester", None);
    fixture
        .cassie
        .execute_sql(
            &writer,
            "CREATE TABLE rr_outside_descriptor (id INT)",
            Vec::new(),
        )
        .expect("outside DDL");

    // Act
    let outside_data = fixture
        .execute("SELECT id FROM rr_outside_descriptor")
        .expect("current descriptor, old data");
    let catalog = fixture.execute("SELECT table_name FROM information_schema.tables WHERE table_name IN ('rr_own_descriptor', 'rr_outside_descriptor') ORDER BY table_name").expect("old plus own rows");
    let failure = fixture.execute("CREATE TABLE rr_later_descriptor (id INT)");
    fixture.execute("ROLLBACK").expect("retire");

    // Assert
    assert_eq!(outside_data.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        catalog.rows(),
        &[vec![Value::String("rr_own_descriptor".into())]]
    );
    assert!(matches!(
        failure,
        Err(CassieError::Transaction {
            kind: cassie::app::TransactionErrorKind::SerializationFailure,
            ..
        })
    ));
}

#[test]
fn should_share_the_command_catalog_with_a_statement_mutation_fork() {
    // Arrange
    let fixture = sql_fixture(
        "rr-fork-catalog",
        &["CREATE TABLE rr_catalog_counts (counted BIGINT)"],
    );
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");
    fixture
        .execute("CREATE TABLE rr_catalog_source (id INT)")
        .expect("own DDL");

    // Act
    fixture.execute("INSERT INTO rr_catalog_counts SELECT count(*) FROM information_schema.tables WHERE table_name = 'rr_catalog_source'").expect("own catalog in DML source");
    let source = fixture
        .execute("SELECT counted FROM rr_catalog_counts")
        .expect("count");
    fixture.execute("ROLLBACK").expect("retire");

    // Assert
    assert_eq!(source.rows(), &[vec![Value::Int64(1)]]);
}

#[test]
fn should_preserve_the_catalog_boundary_for_ctas_after_earlier_own_ddl() {
    // Arrange
    let fixture = sql_fixture("rr-nested-ctas-catalog", &[]);
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");
    fixture
        .execute("CREATE TABLE rr_prior_catalog (id INT)")
        .expect("earlier own DDL");

    // Act
    fixture.execute("CREATE TABLE rr_later_catalog AS SELECT count(*) AS prior_count FROM information_schema.tables WHERE table_name IN ('rr_prior_catalog', 'rr_later_catalog')").expect("CTAS in existing overlay");
    let observed = fixture
        .execute("SELECT prior_count FROM rr_later_catalog")
        .expect("source count");
    fixture.execute("COMMIT").expect("publish both tables");
    let published = fixture.execute("SELECT count(*) FROM information_schema.tables WHERE table_name IN ('rr_prior_catalog', 'rr_later_catalog')").expect("current published catalog");

    // Assert
    assert_eq!(observed.rows(), &[vec![Value::Int64(1)]]);
    assert_eq!(published.rows(), &[vec![Value::Int64(2)]]);
}
