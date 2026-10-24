use std::collections::{HashMap, HashSet};

use super::{function, registry, FunctionArity, FunctionId, FunctionReturnType};

#[test]
fn should_define_one_metadata_mapping_per_function_id() {
    // Arrange
    let functions = registry();
    let mut names = HashSet::new();
    let mut mappings = HashMap::new();

    // Act
    for metadata in functions {
        assert!(names.insert(metadata.name.to_ascii_lowercase()));
        let mapping = (metadata.return_type, metadata.arity);
        if let Some(existing) = mappings.insert(metadata.id, mapping) {
            assert_eq!(
                existing, mapping,
                "divergent metadata for {:?}",
                metadata.id
            );
        }
    }

    // Assert
    assert!(!mappings.is_empty());
    assert!(mappings.values().all(|(return_type, arity)| {
        !matches!(return_type, FunctionReturnType::Unknown)
            || matches!(arity, FunctionArity::Exact(_))
    }));
}

#[test]
fn should_resolve_supported_qualified_function_alias() {
    // Arrange
    let qualified_name = "pg_catalog.pg_show_all_settings";

    // Act
    let metadata = function(qualified_name).expect("qualified alias should resolve");

    // Assert
    assert_eq!(metadata.id, FunctionId::PgShowAllSettings);
    assert!(metadata.bypass_arity_validation);
}

#[test]
fn should_preserve_named_void_identities_for_temporal_sleep_functions() {
    // Arrange
    let sleep_functions = [
        ("pg_sleep", FunctionId::PgSleep),
        ("pg_sleep_for", FunctionId::PgSleepFor),
        ("pg_sleep_until", FunctionId::PgSleepUntil),
    ];

    // Act
    // Assert
    for (name, expected_id) in sleep_functions {
        let metadata = function(name).expect("registered temporal sleep function");
        assert_eq!(metadata.id, expected_id, "{name} identity");
        assert_eq!(
            metadata.return_type,
            FunctionReturnType::Text,
            "{name} carrier"
        );
        assert_eq!(metadata.arity, FunctionArity::Exact(1), "{name} arity");
        assert!(
            registry()
                .iter()
                .any(|registered| registered.id == expected_id),
            "{name} appears in the public function registry"
        );
    }
}
