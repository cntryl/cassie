//! The read campaign must never mutate/open its original fixture.
#[path = "../../benches/tier3_replay_durability/read_campaign/read_oracle.rs"]
mod read_oracle;
#[path = "../../benches/tier3_replay_durability/read_campaign/store_copy.rs"]
mod store_copy;
use std::path::PathBuf;
use store_copy::{fingerprint, load_profile, sha256_file, OwnedCopy, ProfileFiles, SourceIdentity};

struct Fixture {
    root: PathBuf,
    source: PathBuf,
    files: ProfileFiles,
    api: SourceIdentity,
    sql: SourceIdentity,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("cassie-read-copy-test-{}", uuid::Uuid::new_v4()));
    let source = root.join("source");
    std::fs::create_dir_all(source.join("nested")).unwrap();
    std::fs::write(source.join("nested/data"), b"original").unwrap();
    let api = SourceIdentity {
        commit: "a".repeat(40),
        tree: "b".repeat(40),
    };
    let sql = SourceIdentity {
        commit: "c".repeat(40),
        tree: "d".repeat(40),
    };
    let result_path = root.join("result.json");
    let log_path = root.join("producer.log");
    let closure_path = root.join("producer-exit.json");
    let record = serde_json::json!({
        "source": format!("{};tree={};profile=test;feature=replay-benchmark-observation", api.commit, api.tree),
        "mode": "local_strict", "shape": "indexed", "workload": "apply",
        "batch_size": 1000, "batch_count": 100, "events": 100000,
        "query_memory_budget_bytes": 67108864, "query_timeout_ms": 120000,
        "storage_path": source, "recovery_boundary": "clean shutdown then constructor and startup; verification excluded"
    });
    let bytes = serde_json::to_vec(&record).unwrap();
    std::fs::write(&result_path, &bytes).unwrap();
    std::fs::write(&log_path, &bytes).unwrap();
    let result_sha256 = sha256_file(&result_path).unwrap();
    let log_sha256 = sha256_file(&log_path).unwrap();
    let closure = serde_json::json!({ "writer": api, "exit_code": 0, "clean_closed": true,
        "storage_path": source, "result_sha256": result_sha256, "log_sha256": log_sha256 });
    std::fs::write(&closure_path, serde_json::to_vec(&closure).unwrap()).unwrap();
    let files = ProfileFiles {
        result_path,
        result_sha256,
        log_path,
        log_sha256,
        closure_sha256: sha256_file(&closure_path).unwrap(),
        closure_path,
        source_fingerprint: fingerprint(&source).unwrap(),
    };
    Fixture {
        root,
        source,
        files,
        api,
        sql,
    }
}

#[test]
fn should_preserve_original_bytes_when_the_reader_mutates_its_distinct_copy() {
    // Arrange
    let fixture = fixture();
    let checked = load_profile(&fixture.files, &fixture.api, &fixture.sql).unwrap();
    let before = fingerprint(&fixture.source).unwrap();
    assert_eq!(checked.writer, fixture.api);

    // Act
    let copy = OwnedCopy::prepare(&checked).unwrap();
    assert_eq!(fingerprint(copy.path()).unwrap(), before);
    std::fs::write(
        copy.path().join("nested/data"),
        b"reader writes only its copy",
    )
    .unwrap();
    copy.verify_original().unwrap();

    // Assert
    assert_ne!(copy.path(), fixture.source);
    assert_eq!(
        std::fs::read(fixture.source.join("nested/data")).unwrap(),
        b"original"
    );
    assert_eq!(fingerprint(&fixture.source).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_ne!(
            std::fs::metadata(copy.path().join("nested/data"))
                .unwrap()
                .ino(),
            std::fs::metadata(fixture.source.join("nested/data"))
                .unwrap()
                .ino()
        );
    }
    let owned = copy.path().to_owned();
    drop(copy);
    assert!(!owned.exists());
    assert!(fixture.source.exists());
}

#[test]
fn should_reject_a_foreign_writer_even_when_its_artifact_hashes_match() {
    // Arrange
    let mut fixture = fixture();
    fixture.api.commit = "e".repeat(40);
    let before = fingerprint(&fixture.source).unwrap();

    // Act
    let result = load_profile(&fixture.files, &fixture.api, &fixture.sql);

    // Assert
    assert!(result.is_err());
    assert_eq!(fingerprint(&fixture.source).unwrap(), before);
}

#[test]
fn should_reject_missing_or_unsuccessful_clean_close_evidence_before_copying() {
    for key in ["missing", "exit_code", "clean_closed", "result_sha256"] {
        // Arrange
        let mut fixture = fixture();
        let before = fingerprint(&fixture.source).unwrap();
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&fixture.files.closure_path).unwrap()).unwrap();
        match key {
            "missing" => {
                std::fs::remove_file(&fixture.files.closure_path).unwrap();
            }
            "exit_code" => receipt[key] = serde_json::json!(101),
            "clean_closed" => receipt[key] = serde_json::json!(false),
            _ => receipt[key] = serde_json::json!("0".repeat(64)),
        }
        if key != "missing" {
            std::fs::write(
                &fixture.files.closure_path,
                serde_json::to_vec(&receipt).unwrap(),
            )
            .unwrap();
            fixture.files.closure_sha256 = sha256_file(&fixture.files.closure_path).unwrap();
        }

        // Act
        let result = load_profile(&fixture.files, &fixture.api, &fixture.sql);

        // Assert
        assert!(result.is_err(), "closure control {key}");
        assert_eq!(fingerprint(&fixture.source).unwrap(), before);
    }
}

#[cfg(unix)]
#[test]
fn should_reject_a_symlink_inside_an_otherwise_verified_closed_source() {
    // Arrange
    let fixture = fixture();
    let checked = load_profile(&fixture.files, &fixture.api, &fixture.sql).unwrap();
    std::os::unix::fs::symlink(
        fixture.source.join("nested/data"),
        fixture.source.join("link"),
    )
    .unwrap();

    // Act
    let result = OwnedCopy::prepare(&checked);

    // Assert
    assert!(result.unwrap_err().contains("symlink"));
    assert_eq!(
        std::fs::read(fixture.source.join("nested/data")).unwrap(),
        b"original"
    );
}

#[test]
fn should_reject_a_changed_original_after_the_owned_reader_copy_is_prepared() {
    // Arrange
    let fixture = fixture();
    let checked = load_profile(&fixture.files, &fixture.api, &fixture.sql).unwrap();
    let copy = OwnedCopy::prepare(&checked).unwrap();

    // Act
    std::fs::write(
        fixture.source.join("nested/data"),
        b"changed by another owner",
    )
    .unwrap();
    let result = copy.verify_original();

    // Assert
    assert!(result.is_err());
    assert_eq!(
        std::fs::read(copy.path().join("nested/data")).unwrap(),
        b"original"
    );
}

#[test]
fn should_reject_a_mismatched_raw_receipt_or_original_fingerprint() {
    // Arrange
    let mut fixture = fixture();
    fixture.files.result_sha256 = "0".repeat(64);

    // Act
    let receipt = load_profile(&fixture.files, &fixture.api, &fixture.sql);
    fixture.files.result_sha256 = sha256_file(&fixture.files.result_path).unwrap();
    fixture.files.source_fingerprint = "0".repeat(64);
    let checked = load_profile(&fixture.files, &fixture.api, &fixture.sql).unwrap();
    let source = OwnedCopy::prepare(&checked);

    // Assert
    assert!(receipt.is_err());
    assert!(source.is_err());
    assert_eq!(
        std::fs::read(fixture.source.join("nested/data")).unwrap(),
        b"original"
    );
}

#[test]
fn should_reject_duplicate_row_ids_despite_matching_bucket_cardinality() {
    // Arrange
    use cassie::types::Value;
    let rows = vec![
        vec![
            Value::String("same-id".into()),
            Value::String("title-0".into()),
        ],
        vec![
            Value::String("same-id".into()),
            Value::String("title-0".into()),
        ],
    ];
    let mut round_ids = std::collections::BTreeSet::new();

    // Act
    let duplicate = read_oracle::validate_bucket(&rows, 0, 2, &mut round_ids);

    // Assert
    assert!(duplicate.unwrap_err().contains("duplicate"));
    let positive = vec![
        vec![
            Value::String("first".into()),
            Value::String("title-0".into()),
        ],
        vec![
            Value::String("second".into()),
            Value::String("title-0".into()),
        ],
    ];
    round_ids.clear();
    read_oracle::validate_bucket(&positive, 0, 2, &mut round_ids).unwrap();
    assert_eq!(round_ids.len(), 2);
    let across_buckets = vec![vec![
        Value::String("first".into()),
        Value::String("title-1".into()),
    ]];
    assert!(read_oracle::validate_bucket(&across_buckets, 1, 1, &mut round_ids).is_err());
}

#[test]
fn should_reject_alias_paths_reusing_the_same_original_before_copying() {
    // Arrange
    let fixture = fixture();
    let before = fingerprint(&fixture.source).unwrap();
    let mut stores = std::collections::BTreeSet::new();
    store_copy::claim_original(&fixture.source, &mut stores).unwrap();

    // Act
    let alias = store_copy::claim_original(&fixture.source.join("nested/.."), &mut stores);

    // Assert
    assert!(alias.unwrap_err().contains("duplicate"));
    assert_eq!(stores.len(), 1);
    assert_eq!(fingerprint(&fixture.source).unwrap(), before);
}
