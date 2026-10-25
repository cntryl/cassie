//! PostgreSQL 18 catalog information behavior tracked by #817.

#[path = "../catalog_lookup.rs"]
mod catalog_lookup;
#[path = "../../indexes/index_property_functions.rs"]
mod index_property_functions;
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

fn created_object_catalog_fixture(label: &str) -> crate::support_sql_fixture::SqlFixture {
    // Arrange
    let fixture = sql_fixture(label, &[]);
    for sql in [
        "CREATE SCHEMA issue_817_catalog_probe",
        "CREATE TABLE issue_817_catalog_probe.items (\
            id SERIAL PRIMARY KEY, \
            amount NUMERIC NOT NULL DEFAULT 1.25, \
            CONSTRAINT issue_817_amount_positive CHECK (amount > 0))",
        "CREATE INDEX issue_817_amount_idx ON issue_817_catalog_probe.items(amount)",
    ] {
        fixture
            .execute(sql)
            .unwrap_or_else(|error| panic!("setup statement failed: {sql}: {error:?}"));
    }
    fixture.cassie.startup().expect("startup");
    fixture
}

#[test]
fn should_report_identity_column_metadata_in_information_schema() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-information-schema-column-generation",
        &["CREATE TABLE issue_817_identity_columns (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT column_name, is_identity, identity_generation, is_generated \
             FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'issue_817_identity_columns' \
             ORDER BY ordinal_position",
        )
        .expect("read identity and generation metadata");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("id".to_owned()),
            Value::String("NO".to_owned()),
            Value::Null,
            Value::String("NEVER".to_owned()),
        ]]
    );
}

include!("semantics.rs");

include!("../../constraints/catalog_information/nulls.rs");

include!("../../constraints/catalog_information/semantics.rs");

include!("../../descriptions/catalog_information/semantics.rs");

include!("../../identity_privileges/catalog_information/semantics.rs");

include!("../../indexes/catalog_information/semantics.rs");

include!("../../partitions/catalog_information/semantics.rs");

include!("../../policies/catalog_information/semantics.rs");

include!("../../relations/catalog_information/metadata.rs");

include!("../../relations/catalog_information/semantics.rs");

include!("../../routines/catalog_information/parameters.rs");

include!("../../routines/catalog_information/semantics.rs");

include!("../../sequences/catalog_information/semantics.rs");

include!("../../sizes/catalog_information/semantics.rs");

include!("../../system_information/catalog_information/semantics.rs");

include!("../../triggers/catalog_information/semantics.rs");

include!("../../types/catalog_information/semantics.rs");
