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
