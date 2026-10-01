use std::{collections::BTreeSet, process::Command};

fn workspace_metadata() -> serde_json::Value {
    let output = Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1", "--no-deps"])
        .output()
        .expect("cargo metadata should run from the workspace");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata should be valid JSON")
}

#[test]
fn should_assign_every_workspace_integration_test_to_one_backend_ci_shard() {
    // Arrange
    let metadata = workspace_metadata();
    let shards: serde_json::Value =
        serde_json::from_str(include_str!("../.github/backend-test-shards.json"))
            .expect("backend test shard configuration should be valid JSON");

    // Act
    let expected = metadata["packages"]
        .as_array()
        .expect("cargo metadata should include packages")
        .iter()
        .flat_map(|package| {
            package["targets"]
                .as_array()
                .expect("packages should include targets")
                .iter()
        })
        .filter(|target| {
            target["test"].as_bool() == Some(true)
                && target["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "test"))
        })
        .map(|target| {
            target["name"]
                .as_str()
                .expect("test targets should have names")
                .to_string()
        })
        .collect::<Vec<_>>();
    let actual = shards["include"]
        .as_array()
        .expect("backend shards should be an include array")
        .iter()
        .flat_map(|shard| {
            let arguments = shard["cargo_test_args"]
                .as_array()
                .expect("every shard should define Cargo test arguments");
            arguments
                .windows(2)
                .filter(|pair| pair[0] == "--test")
                .map(|pair| {
                    pair[1]
                        .as_str()
                        .expect("--test should be followed by a target name")
                        .to_string()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    // Assert
    let mut expected = expected;
    let mut actual_sorted = actual.clone();
    expected.sort_unstable();
    actual_sorted.sort_unstable();
    assert_eq!(
        expected.len(),
        expected.iter().collect::<BTreeSet<_>>().len(),
        "workspace integration test targets must have unique names"
    );
    assert_eq!(
        actual.len(),
        actual.iter().collect::<BTreeSet<_>>().len(),
        "an integration test target must appear in exactly one shard"
    );
    assert_eq!(actual_sorted, expected);
}

#[test]
fn should_preserve_workspace_builtin_test_coverage() {
    // Arrange
    let metadata = workspace_metadata();
    let shards: serde_json::Value =
        serde_json::from_str(include_str!("../.github/backend-test-shards.json"))
            .expect("backend test shard configuration should be valid JSON");
    let arguments = shards["include"]
        .as_array()
        .expect("backend shards should be an include array")
        .iter()
        .flat_map(|shard| {
            shard["cargo_test_args"]
                .as_array()
                .expect("every shard should define Cargo test arguments")
        })
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>();

    // Act
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata should include packages");
    let expected_binaries = packages
        .iter()
        .flat_map(|package| {
            package["targets"]
                .as_array()
                .expect("packages should include targets")
                .iter()
        })
        .filter(|target| {
            target["test"].as_bool() == Some(true)
                && target["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        })
        .map(|target| {
            target["name"]
                .as_str()
                .expect("binary targets should have names")
                .to_string()
        })
        .collect::<BTreeSet<_>>();
    let actual_binaries = arguments
        .windows(2)
        .filter(|pair| pair[0] == "--bin")
        .map(|pair| pair[1].to_string())
        .collect::<BTreeSet<_>>();
    let expects_library = packages
        .iter()
        .flat_map(|package| {
            package["targets"]
                .as_array()
                .expect("packages should include targets")
                .iter()
        })
        .any(|target| {
            target["test"].as_bool() == Some(true)
                && target["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.iter().any(|kind| kind == "lib"))
        });
    let expects_docs = packages
        .iter()
        .flat_map(|package| {
            package["targets"]
                .as_array()
                .expect("packages should include targets")
                .iter()
        })
        .any(|target| target["doctest"].as_bool() == Some(true));

    // Assert
    if expects_library {
        assert!(
            arguments.contains(&"--lib"),
            "backend CI must run library unit tests"
        );
    }
    assert_eq!(actual_binaries, expected_binaries);
    if expects_docs {
        assert!(
            arguments.contains(&"--doc"),
            "backend CI must run documentation tests"
        );
    }
}

#[test]
fn should_gate_backend_status_on_required_lanes() {
    // Arrange
    let workflow = include_str!("../.github/workflows/ci-backend.yml");

    // Act
    let loads_the_checked_shard_matrix =
        workflow.contains("matrix: ${{ fromJSON(needs.backend-test-matrix.outputs.matrix) }}");
    let reads_the_coverage_checked_matrix = workflow.contains(".github/backend-test-shards.json");
    let aggregates_quality_and_test_shards =
        workflow.contains("needs: [backend-quality, backend-tests]");
    let fails_when_any_lane_fails = workflow.contains("needs.backend-tests.result")
        && workflow.contains("needs.backend-quality.result");
    let runs_the_selected_workspace_targets = workflow
        .contains("cargo test --locked --workspace ${{ join(matrix.cargo_test_args, ' ') }}");
    let keeps_the_backend_status_name = workflow.contains("name: backend\n");

    // Assert
    assert!(loads_the_checked_shard_matrix);
    assert!(reads_the_coverage_checked_matrix);
    assert!(aggregates_quality_and_test_shards);
    assert!(fails_when_any_lane_fails);
    assert!(runs_the_selected_workspace_targets);
    assert!(keeps_the_backend_status_name);
}
