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
