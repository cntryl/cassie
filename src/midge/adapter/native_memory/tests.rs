use super::NativeMemoryConfiguration;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use cntryl_midge::{MemoryBudget, OpenOptions};

#[test]
fn should_preserve_native_defaults_for_an_unconfigured_memory_instance() {
    // Arrange
    let expected = NativeMemoryConfiguration::from_options(
        &OpenOptions::in_memory().build().expect("native defaults"),
    );

    // Act
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory instance");
    let actual = cassie.midge.native_memory_configuration();

    // Assert
    assert_eq!(*actual, expected);
    assert_eq!(actual.mode, "memory");
}

#[test]
fn should_forward_explicit_memory_limits_to_the_native_builder() {
    // Arrange
    let config = CassieRuntimeConfig {
        storage_memory_budget_bytes: Some(64 * 1024 * 1024),
        storage_memtable_size_bytes: Some(16 * 1024 * 1024),
        ..CassieRuntimeConfig::default()
    };
    let options = OpenOptions::in_memory()
        .memory_budget(MemoryBudget::Bytes(64 * 1024 * 1024))
        .with_memtable_size_limit(16 * 1024 * 1024)
        .build()
        .expect("native options");
    let expected = NativeMemoryConfiguration::from_options(&options);

    // Act
    let cassie = Cassie::new_in_memory_with_config(config).expect("configured memory instance");
    let actual = cassie
        .midge
        .native_memory_diagnostics()
        .expect("native runtime readback");

    // Assert
    assert_eq!(actual.configuration, expected);
    assert_eq!(actual.configuration.mode, "memory");
    assert_eq!(actual.configuration.total, 64 * 1024 * 1024);
    assert_eq!(actual.configuration.memtable_limit, 16 * 1024 * 1024);
    assert!(!actual.runtime.write_stalled);
}

#[test]
fn should_retain_native_rejection_of_an_incompatible_memory_profile() {
    // Arrange
    let config = CassieRuntimeConfig {
        storage_memory_budget_bytes: Some(1024 * 1024 * 1024),
        storage_memtable_size_bytes: Some(512 * 1024 * 1024),
        ..CassieRuntimeConfig::default()
    };

    // Act
    let error = Cassie::new_in_memory_with_config(config)
        .err()
        .expect("two generations and pools exceed one GiB");

    // Assert
    assert!(error.to_string().contains("memtables"), "{error}");
}

#[test]
fn should_ignore_memory_only_settings_for_an_explicit_local_instance() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-native-memory-scope-{}",
        uuid::Uuid::new_v4()
    ));
    let config = CassieRuntimeConfig {
        storage_memory_budget_bytes: Some(1024 * 1024 * 1024),
        storage_memtable_size_bytes: Some(512 * 1024 * 1024),
        ..CassieRuntimeConfig::default()
    };

    // Act
    let cassie = Cassie::new_local_with_data_dir_and_config(&path, config)
        .expect("memory-only settings do not change local construction");
    let actual = cassie.midge.native_memory_configuration();

    // Assert
    assert_eq!(actual.mode, "local");
    drop(cassie);
    std::fs::remove_dir_all(path).expect("local fixture cleanup");
}
