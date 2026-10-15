//! Direct catalog and system-information lookups tracked by #817.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_report_select_privilege_for_the_pg_class_relation_name_column() {
    // Arrange
    let fixture = sql_fixture("issue-817-column-privilege", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.has_column_privilege( \
                'pg_catalog.pg_class', 'relname', 'SELECT')",
        )
        .expect("check catalog column privilege");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}

include!("../relations/catalog_lookup.rs");

include!("../routines/catalog_lookup.rs");

include!("../sequences/catalog_lookup.rs");

include!("../sizes/catalog_lookup.rs");

include!("../system_information/catalog_lookup.rs");
