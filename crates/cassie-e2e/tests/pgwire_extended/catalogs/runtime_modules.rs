//! Loaded native-module rows remain empty for the current Cassie profile.
use crate::support_bind_planning as planning;
use crate::support_runtime_information_wire;

#[test]
fn should_report_module_shape_0_given_no_native_plugin_loader_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_get_loaded_modules()";
    // Act
    let batches = planning::pruned("runtime-modules-0", sql);
    // Assert
    support_runtime_information_wire::assert_modules(&batches);
}

#[test]
fn should_report_module_shape_1_given_no_native_plugin_loader_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT module_name,version,file_name FROM pg_catalog.pg_get_loaded_modules() ORDER BY module_name";
    // Act
    let batches = planning::pruned("runtime-modules-1", sql);
    // Assert
    support_runtime_information_wire::assert_modules(&batches);
}

#[test]
fn should_report_module_shape_2_given_no_native_plugin_loader_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_get_loaded_modules() LIMIT 1";
    // Act
    let batches = planning::pruned("runtime-modules-2", sql);
    // Assert
    support_runtime_information_wire::assert_modules(&batches);
}

#[test]
fn should_report_module_shape_3_given_no_native_plugin_loader_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM pg_get_loaded_modules() WHERE module_name IS NOT NULL";
    // Act
    let batches = planning::pruned("runtime-modules-3", sql);
    // Assert
    support_runtime_information_wire::assert_modules(&batches);
}

#[test]
fn should_report_module_shape_4_given_no_native_plugin_loader_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT * FROM (SELECT * FROM pg_get_loaded_modules()) AS q";
    // Act
    let batches = planning::pruned("runtime-modules-4", sql);
    // Assert
    support_runtime_information_wire::assert_modules(&batches);
}

#[test]
fn should_report_module_shape_5_given_no_native_plugin_loader_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_get_loaded_modules()) SELECT * FROM q";
    // Act
    let batches = planning::pruned("runtime-modules-5", sql);
    // Assert
    support_runtime_information_wire::assert_modules(&batches);
}

#[test]
fn should_preserve_alias_given_loaded_module_columns_when_qualified_names_are_selected() {
    // Arrange
    let sql = "WITH q AS (SELECT * FROM pg_catalog.pg_get_loaded_modules()) SELECT q.module_name,q.version,q.file_name FROM q";
    // Act
    let batches = planning::pruned("runtime-module-alias", sql);
    // Assert
    support_runtime_information_wire::assert_modules(&batches);
}

#[test]
fn should_preserve_columns_given_loaded_module_columns_when_qualified_names_are_selected() {
    // Arrange
    let sql = "SELECT m.module_name,m.version,m.file_name FROM pg_get_loaded_modules() AS m";
    // Act
    let batches = planning::pruned("runtime-module-columns", sql);
    // Assert
    support_runtime_information_wire::assert_modules(&batches);
}
