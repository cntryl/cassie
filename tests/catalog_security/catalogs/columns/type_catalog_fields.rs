//! Finite relation and type catalog fields selected for #817/#828.

use crate::support_sql_fixture::sql_fixture;
use cassie::app::{Cassie, CassieSession};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

fn catalog_column_names(
    fixture: &crate::support_sql_fixture::SqlFixture,
    relation: &str,
) -> Vec<String> {
    let sql = format!("SELECT * FROM pg_catalog.{relation} LIMIT 0");
    fixture
        .execute(&sql)
        .unwrap_or_else(|error| panic!("read pg_catalog.{relation} descriptor: {error}"))
        .columns()
        .iter()
        .map(|column| column.name.clone())
        .collect()
}

#[test]
fn should_expose_selected_relation_and_type_fields_with_non_domain_defaults() {
    // Arrange
    let fixture = sql_fixture("issue-817-type-catalog-fields", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE TABLE issue_817_rowtype_probe (value INTEGER)")
        .expect("create a table before composite row types are supported");
    fixture
        .execute("CREATE VIEW issue_817_rowtype_view AS SELECT value FROM issue_817_rowtype_probe")
        .expect("create a view before composite row types are supported");
    fixture
        .execute("CREATE INDEX issue_817_rowtype_idx ON issue_817_rowtype_probe (value)")
        .expect("create an ordinary index");

    // Act
    let class_columns = catalog_column_names(&fixture, "pg_class");
    let type_columns = catalog_column_names(&fixture, "pg_type");
    let relation_types = fixture
        .execute(
            "SELECT relname, relkind, reltype FROM pg_catalog.pg_class \
             WHERE relname IN ('issue_817_rowtype_probe', 'issue_817_rowtype_view', 'issue_817_rowtype_idx') \
             ORDER BY relname",
        )
        .expect("read selected relation type identities");
    let domain_fields = fixture
        .execute(
            "SELECT typname, typnotnull, typtypmod FROM pg_catalog.pg_type \
             WHERE oid IN (23, 25) ORDER BY oid",
        )
        .expect("read non-domain defaults for selected domain fields");

    // Assert
    assert_eq!(
        class_columns,
        [
            "oid",
            "relname",
            "relkind",
            "relam",
            "relnamespace",
            "relnamespace_oid",
            "reltype",
            "relowner",
            "relhasindex",
            "relpersistence",
            "reltuples",
            "relacl",
            "reloptions",
        ]
        .map(str::to_owned)
    );
    assert_eq!(
        type_columns,
        [
            "oid",
            "typname",
            "typnamespace",
            "typlen",
            "typbyval",
            "typtype",
            "typcategory",
            "typelem",
            "typarray",
            "typbasetype",
            "typnotnull",
            "typtypmod",
            "typrelid",
        ]
        .map(str::to_owned)
    );
    assert_eq!(
        relation_types.rows(),
        vec![
            vec![
                Value::String("issue_817_rowtype_idx".to_owned()),
                Value::String("i".to_owned()),
                Value::Int64(0),
            ],
            vec![
                Value::String("issue_817_rowtype_probe".to_owned()),
                Value::String("r".to_owned()),
                Value::Int64(0),
            ],
            vec![
                Value::String("issue_817_rowtype_view".to_owned()),
                Value::String("v".to_owned()),
                Value::Int64(0),
            ],
        ]
    );
    assert_eq!(
        domain_fields.rows(),
        vec![
            vec![
                Value::String("int4".to_owned()),
                Value::Bool(false),
                Value::Int64(-1),
            ],
            vec![
                Value::String("text".to_owned()),
                Value::Bool(false),
                Value::Int64(-1),
            ],
        ]
    );
}

#[test]
fn should_preserve_relhasindex_after_dropping_last_index_through_restart() {
    // Arrange
    let directory = crate::support_catalog_directory::CatalogDirectory::new(
        "issue-817-relhasindex-restart",
    );
    let config = crate::support_sql::with_test_root_password(CassieRuntimeConfig::default());
    let query = "SELECT relhasindex FROM pg_catalog.pg_class \
                 WHERE relname = 'issue_817_relhasindex_rows'";
    let tables_query = "SELECT hasindexes FROM pg_catalog.pg_tables \
                        WHERE tablename = 'issue_817_relhasindex_rows'";

    let after_drop;
    let tables_after_drop;
    {
        let cassie = Cassie::new_with_data_dir_and_config(&directory.0, config.clone())
            .expect("create the local catalog instance");
        cassie.startup().expect("start the local catalog instance");
        let session = cassie.create_session("root", None);
        for sql in [
            "CREATE TABLE issue_817_relhasindex_rows (value INTEGER)",
            "CREATE INDEX issue_817_relhasindex_idx ON issue_817_relhasindex_rows (value)",
            "DROP INDEX issue_817_relhasindex_idx",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("catalog lifecycle statement failed: {sql}: {error}"));
        }
        after_drop = cassie
            .execute_sql(&session, query, vec![])
            .expect("read the historical index flag after drop");
        tables_after_drop = cassie
            .execute_sql(&session, tables_query, vec![])
            .expect("read the matching pg_tables index flag after drop");
        drop(session);
        cassie.shutdown();
    }

    let reopened = Cassie::new_with_data_dir_and_config(&directory.0, config)
        .expect("reopen the local catalog instance");
    reopened.startup().expect("start the reopened instance");
    let session: CassieSession = reopened.create_session("root", None);
    let after_restart = reopened
        .execute_sql(&session, query, vec![])
        .expect("read the historical index flag after restart");
    let tables_after_restart = reopened
        .execute_sql(&session, tables_query, vec![])
        .expect("read the matching pg_tables index flag after restart");

    // Assert
    assert_eq!(after_drop.rows(), vec![vec![Value::Bool(true)]]);
    assert_eq!(after_restart.rows(), vec![vec![Value::Bool(true)]]);
    assert_eq!(tables_after_drop.rows(), vec![vec![Value::Bool(true)]]);
    assert_eq!(tables_after_restart.rows(), vec![vec![Value::Bool(true)]]);
    drop(session);
    reopened.shutdown();
}

#[test]
fn should_rollback_relhasindex_history_with_transactional_index_ddl() {
    // Arrange
    let fixture = sql_fixture("issue-817-relhasindex-rollback", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE TABLE issue_817_relhasindex_rollback (value INTEGER)")
        .expect("create a relation that never committed an index");

    // Act
    for sql in [
        "BEGIN",
        "CREATE INDEX issue_817_relhasindex_rollback_idx ON issue_817_relhasindex_rollback (value)",
        "DROP INDEX issue_817_relhasindex_rollback_idx",
        "ROLLBACK",
    ] {
        fixture
            .execute(sql)
            .unwrap_or_else(|error| panic!("transactional index statement failed: {sql}: {error}"));
    }
    let relation = fixture
        .execute(
            "SELECT relhasindex FROM pg_catalog.pg_class \
             WHERE relname = 'issue_817_relhasindex_rollback'",
        )
        .expect("read relation index history after rollback");
    let tables_relation = fixture
        .execute(
            "SELECT hasindexes FROM pg_catalog.pg_tables \
             WHERE tablename = 'issue_817_relhasindex_rollback'",
        )
        .expect("read pg_tables index history after rollback");

    // Assert
    assert_eq!(relation.rows(), vec![vec![Value::Bool(false)]]);
    assert_eq!(tables_relation.rows(), vec![vec![Value::Bool(false)]]);
}
