//! Killed-process recovery after removing only the simulated cloud's local cache.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Profile {
    ApiStrict,
    ApiReplayable,
    SqlStrict,
    SqlReplayable,
}

#[test]
fn should_recover_strict_api_acknowledgement_after_simulated_cloud_cache_loss() {
    // Arrange
    let profile = Profile::ApiStrict;

    // Act
    let recovered = run_cache_loss(profile);

    // Assert
    assert_eq!(recovered, 2);
}

#[test]
fn should_reconcile_api_prefix_after_simulated_cloud_cache_loss() {
    // Arrange
    let profile = Profile::ApiReplayable;

    // Act
    let recovered = run_cache_loss(profile);

    // Assert
    assert!(matches!(recovered, 1 | 2));
}

#[test]
fn should_recover_strict_sql_acknowledgement_after_simulated_cloud_cache_loss() {
    // Arrange
    let profile = Profile::SqlStrict;

    // Act
    let recovered = run_cache_loss(profile);

    // Assert
    assert_eq!(recovered, 2);
}

#[test]
fn should_reconcile_sql_prefix_after_simulated_cloud_cache_loss() {
    // Arrange
    let profile = Profile::SqlReplayable;

    // Act
    let recovered = run_cache_loss(profile);

    // Assert
    assert!(matches!(recovered, 1 | 2));
}
