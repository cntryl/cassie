use std::fs;
use std::path::PathBuf;

fn tests_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}

#[test]
fn should_keep_test_sources_top_level_except_support_helpers() {
    // Arrange
    let tests = tests_root();

    // Act
    let nested_test_directories = fs::read_dir(&tests)
        .expect("tests directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| entry.file_name() != "support")
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    // Assert
    assert!(
        nested_test_directories.is_empty(),
        "test sources must be top-level; only setup helpers may be nested: {nested_test_directories:?}"
    );
}
