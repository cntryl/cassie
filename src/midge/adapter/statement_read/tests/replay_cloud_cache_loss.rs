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

#[test]
fn should_refuse_foreign_and_unowned_cache_roots_before_deletion() {
    // Arrange
    let owned = CacheFixture::new();
    let foreign = CacheFixture::new();
    seed_cache_sentinels(&owned);
    seed_cache_sentinels(&foreign);
    let foreign_before = tree_snapshot(&foreign.root).unwrap();

    // Act
    let foreign_error = wipe_local_cache(&owned, &foreign.cache()).unwrap_err();
    std::fs::write(owned.root.join(OWNER_FILE), "another-owner").unwrap();
    let unowned_before = tree_snapshot(&owned.root).unwrap();
    let unowned_error = wipe_local_cache(&owned, &owned.cache()).unwrap_err();

    // Assert
    assert_eq!(foreign_error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(unowned_error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(tree_snapshot(&foreign.root).unwrap(), foreign_before);
    assert_eq!(tree_snapshot(&owned.root).unwrap(), unowned_before);
}

#[cfg(unix)]
#[test]
fn should_refuse_symlinked_cache_before_deletion() {
    // Arrange
    let owned = CacheFixture::new();
    let outside = CacheFixture::new();
    seed_cache_sentinels(&outside);
    let outside_before = tree_snapshot(&outside.root).unwrap();
    std::fs::remove_dir(owned.cache()).unwrap();
    std::os::unix::fs::symlink(&outside.cache(), owned.cache()).unwrap();

    // Act
    let error = wipe_local_cache(&owned, &owned.cache()).unwrap_err();

    // Assert
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(tree_snapshot(&outside.root).unwrap(), outside_before);
}

#[test]
fn should_preserve_remote_hashes_and_retained_source_when_wiping_owned_cache() {
    // Arrange
    let owned = CacheFixture::new();
    seed_cache_sentinels(&owned);
    let remote_before = tree_snapshot(&owned.cache().join("cloud_store")).unwrap();
    let source_before = std::fs::read(owned.root.join(SOURCE_FILE)).unwrap();

    // Act
    wipe_local_cache(&owned, &owned.cache()).unwrap();

    // Assert
    assert_eq!(tree_snapshot(&owned.cache().join("cloud_store")).unwrap(), remote_before);
    assert_eq!(std::fs::read(owned.root.join(SOURCE_FILE)).unwrap(), source_before);
    assert_eq!(std::fs::read_dir(owned.cache()).unwrap().count(), 1);
    assert!(!owned.cache().join("wal").exists());
}
