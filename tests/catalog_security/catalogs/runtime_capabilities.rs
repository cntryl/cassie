//! PostgreSQL server capability and loaded-module reporting.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_report_cassie_profile_capabilities_given_a_started_server_when_module_jit_numa_and_unicode_helpers_are_called(
) {
    // Arrange
    let fixture = sql_fixture("issue-817-runtime-capabilities", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let capabilities = fixture
        .execute(
            "SELECT pg_catalog.pg_jit_available(), \
                    pg_catalog.pg_numa_available(), \
                    pg_catalog.unicode_version()",
        )
        .expect("read server JIT, NUMA, and Unicode capabilities");
    let modules = fixture
        .execute(
            "SELECT module_name, version, file_name \
             FROM pg_catalog.pg_get_loaded_modules() ORDER BY module_name",
        )
        .expect("list dynamically loaded server modules");

    // Assert
    assert_eq!(
        capabilities
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [16, 16, 25]
    );
    // Cassie's profile has no JIT or NUMA runtime and loads no native modules.
    // These values come from the Cassie profile contract, not the reference
    // machine's build options. Unicode data versions vary by runtime profile.
    let [jit_available, numa_available, unicode_version] = capabilities.rows()[0].as_slice() else {
        panic!("one row with the three capability values");
    };
    assert_eq!(jit_available, &Value::Bool(false));
    assert_eq!(numa_available, &Value::Bool(false));
    let Value::String(unicode_version) = unicode_version else {
        panic!("unicode_version returns a text value");
    };
    assert!(
        !unicode_version.is_empty(),
        "unicode_version reports the runtime's Unicode data version"
    );
    assert_eq!(
        modules
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [25, 25, 25]
    );
    assert_eq!(modules.rows(), &[] as &[Vec<Value>]);
}
