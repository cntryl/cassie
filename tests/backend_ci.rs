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

include!("backend_ci/contracts/semantics.rs");
