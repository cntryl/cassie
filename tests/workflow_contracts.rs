use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn should_use_separate_backend_frontend_ci_workflows() {
    // Arrange
    let workflows = repo_root().join(".github/workflows");
    let combined = workflows.join("ci.yml");
    let backend = workflows.join("ci-backend.yml");
    let frontend = workflows.join("ci-frontend.yml");

    // Act
    let split_exists = !combined.exists() && backend.exists() && frontend.exists();
    let backend_contents = fs::read_to_string(backend).unwrap_or_default();
    let frontend_contents = fs::read_to_string(frontend).unwrap_or_default();

    // Assert
    assert!(
        split_exists,
        "CI must use separate backend and frontend workflows"
    );
    assert!(backend_contents.contains("name: CI Backend"));
    assert!(backend_contents.contains("cargo fmt --all -- --check"));
    assert!(backend_contents.contains("cargo clippy --locked"));
    assert!(backend_contents.contains("cargo build --locked"));
    assert!(backend_contents.contains("cargo test --locked"));
    assert!(!backend_contents.contains("pipefail"));
    assert!(!backend_contents.contains("tee "));
    assert!(!backend_contents.contains("upload-artifact"));
    assert!(frontend_contents.contains("name: CI Frontend"));
    assert!(frontend_contents.contains("public/openapi.yml"));
    assert!(frontend_contents.contains("npm run gen:adapters"));
    assert!(frontend_contents.contains("npm run build"));
    assert!(!frontend_contents.contains("pipefail"));
    assert!(!frontend_contents.contains("tee "));
    assert!(!frontend_contents.contains("upload-artifact"));
}
