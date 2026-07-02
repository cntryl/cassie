use std::path::Path;

#[test]
fn should_use_focused_facades_for_central_subsystems() {
    // Arrange
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let monoliths = [
        "src/app.rs",
        "src/executor/executor.rs",
        "src/midge/adapter.rs",
    ];
    let required_modules = [
        "src/app/mod.rs",
        "src/app/state.rs",
        "src/app/session.rs",
        "src/app/error.rs",
        "src/app/cache.rs",
        "src/app/hydration.rs",
        "src/executor/execution/mod.rs",
        "src/executor/execution/entrypoints.rs",
        "src/executor/execution/dispatch.rs",
        "src/executor/execution/cte.rs",
        "src/executor/execution/result.rs",
        "src/midge/adapter/mod.rs",
        "src/midge/adapter/core.rs",
        "src/midge/adapter/transactions.rs",
        "src/midge/adapter/raw_ops.rs",
    ];

    // Act
    let remaining_monoliths = monoliths
        .iter()
        .filter(|path| repo.join(path).exists())
        .copied()
        .collect::<Vec<_>>();
    let missing_modules = required_modules
        .iter()
        .filter(|path| !repo.join(path).exists())
        .copied()
        .collect::<Vec<_>>();

    // Assert
    assert!(
        remaining_monoliths.is_empty(),
        "central subsystem monoliths must be replaced by module facades: {remaining_monoliths:?}"
    );
    assert!(
        missing_modules.is_empty(),
        "central subsystem focused modules are missing: {missing_modules:?}"
    );
}

#[test]
fn should_keep_command_dispatch_split_from_schema_command_details() {
    // Arrange
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dml_command = repo.join("src/executor/execution/dml_command.rs");
    let schema_command = repo.join("src/executor/execution/schema_command.rs");

    // Act
    let dml_command_lines = std::fs::read_to_string(&dml_command)
        .expect("read dml command module")
        .lines()
        .count();

    // Assert
    assert!(
        schema_command.exists(),
        "schema DDL execution should live in a focused schema_command module"
    );
    assert!(
        dml_command_lines < 1_000,
        "dml_command.rs should stay below 1,000 lines after schema command extraction; found {dml_command_lines}"
    );
}

#[test]
fn should_document_architecture_diagrams_with_drift_analysis() {
    // Arrange
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let architecture_doc = repo.join("docs/architecture-diagrams.md");
    let docs_readme = repo.join("docs/README.md");
    let module_organization = repo.join("docs/module-organization.md");

    // Act
    let architecture_doc_exists = architecture_doc.exists();
    let architecture_doc_content = std::fs::read_to_string(&architecture_doc).unwrap_or_default();
    let docs_readme_content = std::fs::read_to_string(&docs_readme).expect("read docs README");
    let module_organization_content =
        std::fs::read_to_string(&module_organization).expect("read module organization doc");

    // Assert
    assert!(
        architecture_doc_exists,
        "docs/architecture-diagrams.md should document module diagrams and drift analysis"
    );
    assert!(
        architecture_doc_content.contains("```mermaid"),
        "architecture diagrams doc should contain Mermaid code blocks"
    );
    assert!(
        architecture_doc_content.contains("## Architecture Drift Analysis"),
        "architecture diagrams doc should include architecture drift analysis"
    );
    assert!(
        docs_readme_content.contains("[Architecture Diagrams](architecture-diagrams.md)"),
        "docs README should link to architecture diagrams"
    );
    assert!(
        module_organization_content.contains("[Architecture Diagrams](architecture-diagrams.md)"),
        "module organization doc should cross-reference architecture diagrams"
    );
}
