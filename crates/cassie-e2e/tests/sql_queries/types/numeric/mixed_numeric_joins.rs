#[test]
fn should_match_loaded_mixed_join_rows() {
    // Arrange
    let fixture = fixture("mixed-loaded", true, false);
    for family in ["int2", "int4", "int8"] {
        for left in [false, true] {
            for reverse in [false, true] {
                // Act
                let observed = execute(&fixture, &query(family, left, reverse, true));

                // Assert
                // Declared id leaves a hidden scan identity outside the LEFT null template;
                // that existing shape truthfully selects the loaded scalar hash path.
                let strategy = if left { "vectorized" } else { "typed_hash" };
                assert_completed(&observed, strategy, family, left);
            }
        }
    }
}

#[test]
fn should_match_streaming_mixed_join_rows() {
    // Arrange
    let fixture = fixture("mixed-streaming", true, false);
    for reverse in [false, true] {
        // Act
        let observed = execute(&fixture, &query("int8", false, reverse, false));

        // Assert
        assert_completed(&observed, "vectorized", "int8", false);
        assert!(
            delta(
                &observed.before,
                &observed.after,
                "read_paths",
                "collection_scans"
            ) > 0
        );
    }
}

#[test]
fn should_preserve_mixed_join_rows_during_merge_selection() {
    // Arrange
    for switching in [false, true] {
        let fixture = fixture("mixed-merge", switching, switching);
        for reverse in [false, true] {
            // Act
            let observed = execute(&fixture, &query("int8", false, reverse, true));

            // Assert
            assert_completed(&observed, "merge", "int8", false);
            assert_eq!(
                delta(
                    &observed.before,
                    &observed.after,
                    "adaptive_candidates",
                    "operator_switch_successes"
                ),
                u64::from(switching)
            );
            if switching {
                assert_eq!(
                    observed.after["adaptive_candidates"]["last_operator_switch_pair"],
                    "vectorized_join_to_merge_join"
                );
                assert_eq!(
                    observed.after["adaptive_candidates"]["last_operator_switch_reason"],
                    "row_threshold_exceeded"
                );
            }
        }
    }
}

#[test]
fn should_avoid_lossy_mixed_join_index_probes() {
    // Arrange
    let fixture = fixture("mixed-index-overlay", true, false);
    fixture
        .execute("CREATE INDEX mix_exact_key ON mix_int8_l (k)")
        .expect("integer index");
    let before_index = fixture.cassie.metrics();
    let indexed = fixture
        .execute("SELECT id FROM mix_int8_l WHERE k = 9007199254740993")
        .expect("exact integer index positive");
    assert_eq!(indexed.rows(), [vec![Value::Int64(1)]]);
    drop(indexed);
    assert!(
        delta(
            &before_index,
            &fixture.cassie.metrics(),
            "read_paths",
            "index_seek_scans"
        ) > 0
    );
    for overlay in [false, true] {
        if overlay {
            fixture.execute("BEGIN").expect("begin overlay");
            // A real staged write preserves the reference bag but disables unsafe index/stream pruning.
            fixture
                .execute("UPDATE mix_int8_l SET k = k WHERE id = 1")
                .expect("stage original exact key");
        }
        for reverse in [false, true] {
            // Act
            let observed = execute(&fixture, &query("int8", false, reverse, false));

            // Assert
            assert_completed(&observed, "typed_hash", "int8", false);
            assert_eq!(
                delta(
                    &observed.before,
                    &observed.after,
                    "read_paths",
                    "index_seek_scans"
                ),
                0
            );
            assert!(
                delta(
                    &observed.before,
                    &observed.after,
                    "read_paths",
                    "collection_scans"
                ) > 0
            );
        }
        if overlay {
            fixture.execute("ROLLBACK").expect("retire overlay");
        }
    }
}
