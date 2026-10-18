#[test]
fn should_retain_float_index_reads_for_mixed_integer_literals() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.adaptive_execution_enabled = false;
    config.limits.parallel_scan_workers = 1;
    let fixture = sql_fixture_with_config("mixed-float-index-literals", FLOAT_INDEX_SEED, config);

    // Act
    let observations = FLOAT_INDEX_CASES
        .iter()
        .map(|(predicate, expected, path)| {
            (
                *predicate,
                *expected,
                *path,
                observe_float_index_read(&fixture, "mixed_float_scan", predicate),
                observe_float_index_read(&fixture, "mixed_float_indexed", predicate),
            )
        })
        .collect::<Vec<_>>();
    drop(fixture);

    // Assert
    for (predicate, expected, path, baseline, indexed) in observations {
        let expected = expected
            .iter()
            .map(|id| vec![Value::Int64(*id)])
            .collect::<Vec<_>>();
        let (mut baseline_rows, baseline_live) = baseline.rows.expect("unindexed mixed comparison");
        let (mut indexed_rows, indexed_live) = indexed.rows.expect("indexed mixed comparison");
        for rows in [&mut baseline_rows, &mut indexed_rows] {
            rows.sort_by_key(|row| match row.first() {
                Some(Value::Int64(id)) => *id,
                _ => panic!("integer row identity"),
            });
        }
        assert_eq!(baseline_rows, expected, "unindexed {predicate}");
        assert_eq!(indexed_rows, baseline_rows, "indexed {predicate}");
        assert!(baseline_live > 0 && indexed_live > 0, "{predicate}");
        assert_eq!((baseline.retired, indexed.retired), (0, 0), "{predicate}");
        assert!(baseline.scans > 0, "actual unindexed scan: {predicate}");
        assert_eq!((baseline.seeks, baseline.ranges), (0, 0), "{predicate}");
        match path {
            "index_seek_scans" => assert!(indexed.seeks > 0, "actual FLOAT seek: {predicate}"),
            "range_scans" => assert!(indexed.ranges > 0, "actual FLOAT range: {predicate}"),
            "collection_scans" => {
                assert!(indexed.scans > 0, "signed-zero fallback: {predicate}");
                assert_eq!((indexed.seeks, indexed.ranges), (0, 0), "{predicate}");
            }
            _ => panic!("known read-path counter"),
        }
    }
}

#[test]
fn should_preserve_quoted_composite_float_index_constraints() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.adaptive_execution_enabled = false;
    config.limits.parallel_scan_workers = 1;
    let fixture =
        sql_fixture_with_config("quoted-composite-float-index", QUOTED_FLOAT_SEED, config);

    // Act
    let observations = QUOTED_FLOAT_CASES
        .iter()
        .map(|(predicate, aliased, expected, path)| {
            let suffix = if *aliased { " f" } else { "" };
            (
                *predicate,
                *expected,
                *path,
                observe_float_index_read(
                    &fixture,
                    &format!("quoted_float_scan{suffix}"),
                    predicate,
                ),
                observe_float_index_read(
                    &fixture,
                    &format!("quoted_float_indexed{suffix}"),
                    predicate,
                ),
            )
        })
        .collect::<Vec<_>>();
    drop(fixture);

    // Assert
    for (predicate, expected, path, baseline, indexed) in observations {
        let expected = expected
            .iter()
            .map(|id| vec![Value::Int64(*id)])
            .collect::<Vec<_>>();
        let (mut baseline_rows, baseline_live) =
            baseline.rows.expect("unindexed compound predicate");
        let (mut indexed_rows, indexed_live) = indexed.rows.expect("indexed compound predicate");
        for rows in [&mut baseline_rows, &mut indexed_rows] {
            rows.sort_by_key(|row| match row.first() {
                Some(Value::Int64(id)) => *id,
                _ => panic!("integer row identity"),
            });
        }
        assert_eq!(baseline_rows, expected, "unindexed {predicate}");
        assert_eq!(indexed_rows, baseline_rows, "indexed {predicate}");
        assert!(baseline_live > 0 && indexed_live > 0, "{predicate}");
        assert_eq!((baseline.retired, indexed.retired), (0, 0), "{predicate}");
        assert!(baseline.scans > 0, "actual unindexed scan: {predicate}");
        assert_eq!(
            (baseline.seeks, baseline.prefixes, baseline.ranges),
            (0, 0, 0),
            "{predicate}"
        );
        match path {
            "prefix" => assert!(indexed.prefixes > 0, "actual composite prefix: {predicate}"),
            "range" => assert!(indexed.ranges > 0, "actual composite range: {predicate}"),
            "scan" => {
                assert!(
                    indexed.scans > 0,
                    "nullable trailing key fallback: {predicate}"
                );
                assert_eq!(
                    (indexed.seeks, indexed.prefixes, indexed.ranges),
                    (0, 0, 0),
                    "{predicate}"
                );
            }
            _ => panic!("known compound read path"),
        }
    }
}

#[test]
fn should_preserve_indexed_mixed_scalar_comparison_completeness() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let fixture = sql_fixture_with_config("mixed-scalar-index-completeness", SEED, config);
    let before_exact = fixture.cassie.metrics();
    let exact = fixture
        .execute("SELECT id,k FROM mix_scalar_index WHERE k = 9007199254740993")
        .expect("genuine exact-index positive");
    let mut exact_rows = exact.rows().to_vec();
    exact_rows.sort_by_key(|row| match row[0] {
        Value::Int64(id) => id,
        _ => panic!("integer row identity"),
    });
    drop(exact);
    let exact_seeks = delta(
        &before_exact,
        &fixture.cassie.metrics(),
        "read_paths",
        "index_seek_scans",
    );
    let mut observations = Vec::new();

    // Act
    for overlay in [false, true] {
        if overlay {
            fixture.execute("BEGIN").expect("real overlay");
            fixture
                .execute("UPDATE mix_scalar_index SET k=9007199254740993 WHERE id=1")
                .expect("stage adjacent integer neighbor");
        }
        for reverse in [false, true] {
            for (parameter, predicate, family) in [
                (
                    Value::Float64(9_007_199_254_740_992.0),
                    if reverse { "$1 = k" } else { "k = $1" },
                    0,
                ),
                (
                    Value::Float64(-9_007_199_254_740_992.0),
                    if reverse { "$1 = k" } else { "k = $1" },
                    1,
                ),
                (
                    Value::Null,
                    if reverse {
                        "CAST($1 AS FLOAT) IS NOT DISTINCT FROM k"
                    } else {
                        "k IS NOT DISTINCT FROM CAST($1 AS FLOAT)"
                    },
                    2,
                ),
            ] {
                let sql =
                    format!("SELECT id,k FROM mix_scalar_index WHERE {predicate} ORDER BY id");
                let before = fixture.cassie.metrics();
                let result = fixture
                    .cassie
                    .execute_sql(&fixture.session, &sql, vec![parameter])
                    .expect("mixed indexed predicate");
                let rows = result.rows().to_vec();
                let live = accounted_memory(&fixture.cassie.metrics());
                drop(result);
                let after = fixture.cassie.metrics();
                observations.push((
                    overlay,
                    family,
                    rows,
                    live,
                    delta(&before, &after, "read_paths", "index_seek_scans"),
                    delta(&before, &after, "read_paths", "collection_scans"),
                    accounted_memory(&after),
                ));
            }
        }
        if overlay {
            fixture.execute("ROLLBACK").expect("retire actual overlay");
        }
    }
    drop(fixture);

    // Assert
    assert!(
        exact_seeks > 0,
        "the same physical integer index has a real successful seek"
    );
    assert_eq!(exact_rows, expected_rows(false, 3));
    for (overlay, family, rows, live, seeks, scans, retired) in observations {
        let expected = expected_rows(overlay, family);
        assert_eq!(rows, expected, "retained PostgreSQL addendum oracle");
        assert!(live > 0);
        assert_eq!(retired, 0);
        if family != 2 {
            assert_eq!(
                seeks, 0,
                "a Float8 equality domain cannot prune to one exact BIGINT key"
            );
            assert!(
                scans > 0,
                "safe fallback actually scans the indexed collection"
            );
        }
    }
}
