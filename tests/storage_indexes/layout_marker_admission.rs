//! Persisted layout declarations cannot be inferred from surviving row bytes.
use super::{support_sql, support_storage_family_snapshot};
use cassie::app::Cassie;
use cassie::midge::adapter::StorageFamily;

#[test]
fn should_reject_initialized_storage_missing_its_marker_without_changing_any_family() {
    // Arrange
    support_sql::use_local_storage();
    for (populated, missing_temp, corrupt_marker) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, true, true),
    ] {
        let path = support_sql::data_dir("missing-persisted-layout-marker");
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        if populated {
            cassie
                .execute_sql(&session, "CREATE TABLE marker_docs (v BIGINT)", vec![])
                .expect("table");
            cassie
                .execute_sql(&session, "INSERT INTO marker_docs VALUES (1)", vec![])
                .expect("row");
        }
        let marker = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Schema, b"")
            .expect("schema entries")
            .into_iter()
            .find_map(|(key, value)| (value == b"cassie-midge-layout-v2").then_some(key))
            .expect("marker");
        if corrupt_marker {
            cassie
                .midge
                .raw_put(StorageFamily::Schema, &marker, b"invalid-layout")
                .expect("corrupt marker only");
        } else {
            cassie
                .midge
                .raw_delete(StorageFamily::Schema, &marker)
                .expect("remove marker only");
        }
        drop((session, cassie));
        if missing_temp {
            support_storage_family_snapshot::remove_empty_temp_family(std::path::Path::new(&path));
        }
        let before = support_storage_family_snapshot::read(std::path::Path::new(&path));

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen storage");
        let result = restarted.startup();

        // Assert
        assert!(
            result.is_err(),
            "initialized markerless storage must fail closed; populated={populated}"
        );
        let expected = if corrupt_marker {
            "incompatible"
        } else {
            "missing"
        };
        assert!(result.unwrap_err().to_string().contains(expected));
        assert!(
            restarted.startup().is_err(),
            "retry cannot repair the marker"
        );
        drop(restarted);
        let after = support_storage_family_snapshot::read(std::path::Path::new(&path));
        assert!(before == after, "admission rejection preserves all catalog/data/temp bytes and family inventory; before={:?} after={:?}", before.keys().collect::<Vec<_>>(), after.keys().collect::<Vec<_>>());
        assert!(!after
            .values()
            .flatten()
            .any(|(_, value)| value == b"cassie-midge-layout-v2"));
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}

#[test]
fn should_initialize_a_genuinely_empty_directory_with_the_current_marker() {
    // Arrange
    support_sql::use_local_storage();
    let path = support_sql::data_dir("fresh-layout-marker");
    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("fresh owner");
    cassie.startup().expect("fresh initialization");
    // Assert
    assert_eq!(
        cassie
            .midge
            .raw_scan_prefix(StorageFamily::Schema, b"")
            .expect("schema entries")
            .iter()
            .filter(|(_, value)| value == b"cassie-midge-layout-v2")
            .count(),
        1
    );
    cassie.startup().expect("idempotent startup");
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_complete_empty_unpublished_fixed_family_initialization() {
    // Arrange
    support_sql::use_local_storage();
    let path = support_sql::data_dir("empty-fixed-layout-inventory");
    support_storage_family_snapshot::create_empty_families(
        std::path::Path::new(&path),
        &["cf0", "cf1"],
    );
    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("owner");
    cassie.startup().expect("complete first initialization");
    // Assert
    assert!(cassie
        .midge
        .raw_scan_prefix(StorageFamily::Schema, b"")
        .expect("schema entries")
        .iter()
        .any(|(_, value)| value == b"cassie-midge-layout-v2"));
    drop(cassie);
    std::fs::remove_dir_all(path).expect("cleanup");
}

#[test]
fn should_reject_unmarked_opaque_or_unknown_empty_families_without_creating_fixed_families() {
    // Arrange
    support_sql::use_local_storage();
    for name in ["db-orphan", "unknown-owner"] {
        let path = support_sql::data_dir("unmarked-opaque-layout-inventory");
        support_storage_family_snapshot::create_empty_families(
            std::path::Path::new(&path),
            &[name],
        );
        let before = support_storage_family_snapshot::read(std::path::Path::new(&path));
        // Act
        let cassie = Cassie::new_with_data_dir(&path).expect("owner");
        let result = cassie.startup();
        // Assert
        assert!(
            result.is_err(),
            "unmarked opaque inventory is not a fresh database"
        );
        assert!(result.unwrap_err().to_string().contains("missing"));
        drop(cassie);
        let after = support_storage_family_snapshot::read(std::path::Path::new(&path));
        assert!(
            before == after,
            "rejection cannot create cf0/cf1 or change existing contents"
        );
        std::fs::remove_dir_all(path).expect("cleanup");
    }
}
