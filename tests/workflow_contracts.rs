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

#[test]
fn should_cancel_superseded_backend_ci_runs_per_branch_or_pull_request() {
    // Arrange
    let workflow = repo_root().join(".github/workflows/ci-backend.yml");

    // Act
    let contents = fs::read_to_string(workflow).expect("backend workflow");

    // Assert
    assert!(contents.contains("concurrency:"));
    assert!(contents.contains("ci-backend-${{ github.event.pull_request.head.ref || github.ref }}"));
    assert!(contents.contains("cancel-in-progress: true"));
    assert!(contents.contains("timeout-minutes: 30"));
}

#[test]
fn should_run_backend_checks_in_simplified_steps() {
    // Arrange
    let workflow = repo_root().join(".github/workflows/ci-backend.yml");

    // Act
    let contents = fs::read_to_string(workflow).expect("backend workflow");

    // Assert
    assert!(contents.contains("- name: Cargo test\n        run: cargo test --no-run --locked"));
    assert!(contents.contains("- name: Run tests\n        run: cargo test --locked"));
    assert!(!contents.contains("cargo test --locked --lib --bins"));
    assert!(!contents.contains("xargs -0 -n1 -P4"));
}

#[test]
fn should_define_opt_in_version_pinned_compatibility_probes() {
    // Arrange
    let workflow = repo_root().join(".github/workflows/compatibility-probes.yml");
    let contract = repo_root().join("docs/compatibility-probe-contract.md");

    // Act
    let workflow_contents = fs::read_to_string(workflow).unwrap_or_default();
    let contract_contents = fs::read_to_string(contract).unwrap_or_default();

    // Assert
    assert!(workflow_contents.contains("workflow_dispatch:"));
    assert!(workflow_contents.contains("COMPATIBILITY_PROBE"));
    assert!(workflow_contents.contains("SQLALCHEMY_VERSION: 2.0.36"));
    assert!(workflow_contents.contains("PSYCOPG_VERSION: 3.2.3"));
    assert!(workflow_contents.contains("PRISMA_VERSION: 6.1.0"));
    assert!(workflow_contents.contains("POSTGRES_CLIENT_IMAGE: postgres:16.6-bookworm"));
    assert!(workflow_contents.contains("if: ${{ always() }}"));
    assert!(!workflow_contents.contains("CASSIE_ADMIN_PASSWORD"));
    assert!(contract_contents.contains("secret-free"));
    assert!(contract_contents.contains("sqlx"));
    assert!(contract_contents.contains("Diesel"));
    assert!(contract_contents.contains("pgAdmin"));
    assert!(contract_contents.contains("DBeaver"));
}

#[test]
fn should_pin_cntryl_tools_from_github_source() {
    // Arrange
    let backend = repo_root().join(".github/workflows/ci-backend.yml");
    let benchmarks = repo_root().join(".github/workflows/benchmarks.yml");
    let contract = repo_root().join("docs/tooling-contract.md");

    // Act
    let backend_contents = fs::read_to_string(backend).expect("backend workflow");
    let benchmark_contents = fs::read_to_string(benchmarks).expect("benchmark workflow");
    let contract_contents = fs::read_to_string(contract).unwrap_or_default();

    // Assert
    for contents in [&backend_contents, &benchmark_contents] {
        assert!(contents.contains("cargo install --git https://github.com/cntryl/tools"));
        assert!(contents.contains("--rev d36dc1c09462a4fd691ed9fdcc4413eb61f0c80c"));
        assert!(contents.contains("--locked cntryl-tools"));
    }
    assert!(contract_contents.contains("GitHub source"));
    assert!(contract_contents.contains("d36dc1c09462a4fd691ed9fdcc4413eb61f0c80c"));
    assert!(!contract_contents.contains("published release required"));
}
