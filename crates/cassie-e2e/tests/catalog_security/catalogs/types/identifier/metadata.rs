//! Remaining PostgreSQL object-identifier cast and wire contracts for #818.

use super::super::support_pgwire as wire;
use super::super::support_sql_fixture::sql_fixture;
use cassie::types::Value;
use std::time::Duration;

#[test]
fn should_preserve_regnamespace_oid_across_rollback_rename_drop() {
    // Arrange
    let fixture = sql_fixture(
        "issue-818-regnamespace-rename-identity",
        &["CREATE SCHEMA issue_818_namespace_before"],
    );
    fixture.cassie.startup().expect("startup");
    let original_oid = fixture
        .execute("SELECT 'issue_818_namespace_before'::regnamespace::oid")
        .expect("read original namespace OID")
        .rows()[0][0]
        .clone();

    // Act
    fixture.execute("BEGIN").expect("begin rollback check");
    fixture
        .execute(
            "ALTER SCHEMA issue_818_namespace_before \
             RENAME TO issue_818_namespace_rolled_back",
        )
        .expect("rename namespace inside a transaction");
    let during_rename = fixture
        .execute("SELECT 'issue_818_namespace_rolled_back'::regnamespace::oid")
        .expect("read namespace OID after transactional rename");
    fixture
        .execute("ROLLBACK")
        .expect("roll back schema rename");
    let after_rollback = fixture
        .execute(
            "SELECT 'issue_818_namespace_before'::regnamespace::oid, \
                    to_regnamespace('issue_818_namespace_rolled_back') IS NULL",
        )
        .expect("resolve original namespace after rollback");
    fixture
        .execute(
            "ALTER SCHEMA issue_818_namespace_before \
             RENAME TO issue_818_namespace_after",
        )
        .expect("rename namespace while retaining its identity");
    let after_rename = fixture
        .execute(
            "SELECT 'issue_818_namespace_after'::regnamespace::oid, \
                    to_regnamespace('issue_818_namespace_before') IS NULL",
        )
        .expect("resolve renamed namespace");
    fixture
        .execute("DROP SCHEMA issue_818_namespace_after")
        .expect("drop the renamed namespace");
    let after_drop = fixture
        .execute("SELECT to_regnamespace('issue_818_namespace_after') IS NULL")
        .expect("return NULL for the dropped namespace");

    // Assert
    assert_eq!(during_rename.rows(), vec![vec![original_oid.clone()]]);
    assert_eq!(
        after_rollback.rows(),
        vec![vec![original_oid.clone(), Value::Bool(true)]]
    );
    assert_eq!(
        after_rename.rows(),
        vec![vec![original_oid, Value::Bool(true)]]
    );
    assert_eq!(after_drop.rows(), vec![vec![Value::Bool(true)]]);
}

include!("semantics.rs");
