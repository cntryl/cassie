//! Exact SQL input identities must survive cross-session compiled and result caches.

use crate::support_parameter_cache_identity::{
    assert_case, assert_orders, counter, diagnostic_hit, run, run_orders, Case,
};

const INTEGERS: [Case; 3] = [
    Case {
        sql: "SELECT $1::BYTEA AS value",
        oids: &[21],
        inputs: &["-1"],
        columns: &[(17, -1)],
        outputs: &[&[255; 2]],
    },
    Case {
        sql: "SELECT $1::BYTEA AS value",
        oids: &[23],
        inputs: &["-1"],
        columns: &[(17, -1)],
        outputs: &[&[255; 4]],
    },
    Case {
        sql: "SELECT $1::BYTEA AS value",
        oids: &[20],
        inputs: &["-1"],
        columns: &[(17, -1)],
        outputs: &[&[255; 8]],
    },
];

const STRINGS: [Case; 2] = [
    Case {
        sql: "SELECT $1 || $2 AS value",
        oids: &[17, 17],
        inputs: &["\\x01", "\\x02"],
        columns: &[(17, -1)],
        outputs: &[&[1, 2]],
    },
    Case {
        sql: "SELECT $1 || $2 AS value",
        oids: &[25, 25],
        inputs: &["\\x01", "\\x02"],
        columns: &[(25, -1)],
        outputs: &[b"\\x01\\x02"],
    },
];

#[test]
fn should_preserve_integer_cast_widths_across_sessions() {
    // Arrange
    let cases = INTEGERS;
    // Act
    let evidence = run_orders(&cases, true);
    // Assert
    assert_orders(&cases, &evidence);
}

#[test]
fn should_preserve_integer_cast_widths_given_disabled_result_cache() {
    // Arrange
    let cases = INTEGERS;
    // Act
    let evidence = run_orders(&cases, false);
    // Assert
    assert_orders(&cases, &evidence);
}

#[test]
fn should_preserve_integer_cast_widths_given_cold_caches() {
    // Arrange
    let cases = INTEGERS;
    // Act
    let evidence = cases.map(|case| run(&[case], false));
    // Assert
    for (evidence, case) in evidence.iter().zip(cases) {
        assert_case(&evidence[0], case);
    }
}

#[test]
fn should_preserve_bytea_concatenation_across_sessions() {
    // Arrange
    let cases = STRINGS;
    // Act
    let evidence = run_orders(&cases, true);
    // Assert
    assert_orders(&cases, &evidence);
}

#[test]
fn should_preserve_bytea_concatenation_given_disabled_result_cache() {
    // Arrange
    let cases = STRINGS;
    // Act
    let evidence = run_orders(&cases, false);
    // Assert
    assert_orders(&cases, &evidence);
}

#[test]
fn should_preserve_bytea_concatenation_given_cold_caches() {
    // Arrange
    let cases = STRINGS;
    // Act
    let evidence = cases.map(|case| run(&[case], false));
    // Assert
    for (evidence, case) in evidence.iter().zip(cases) {
        assert_case(&evidence[0], case);
    }
}

#[test]
fn should_reuse_same_oid_cached_plans_across_sessions() {
    // Arrange
    let cases = [INTEGERS[0]; 2];
    // Act
    let evidence = run(&cases, false);
    // Assert
    for (evidence, case) in evidence.iter().zip(cases) {
        assert_case(evidence, case);
    }
    assert_eq!(
        counter(&evidence[1], "plan_cache", "misses"),
        counter(&evidence[0], "plan_cache", "misses"),
    );
    assert!(
        counter(&evidence[1], "plan_cache", "hits") > counter(&evidence[0], "plan_cache", "hits"),
    );
}

#[test]
fn should_preserve_transaction_result_cache_bypass_across_sessions() {
    // Arrange
    let cases = [STRINGS[0]; 2];
    // Act
    let evidence = run(&cases, true);
    // Assert
    for (evidence, case) in evidence.iter().zip(cases) {
        assert_case(evidence, case);
    }
    assert_eq!(counter(&evidence[1], "execution_result_cache", "hits"), 0);
    assert!(
        evidence[1].metrics["execution_result_cache"]["bypass_reasons"]["active_transaction"]
            .as_u64()
            .expect("wire transaction bypass")
            > 0,
    );
}

#[test]
fn should_isolate_ordered_parameter_signatures_across_sessions() {
    // Arrange
    let cases = [
        Case {
            sql: "SELECT $1 AS a, $2 AS b",
            oids: &[21, 20],
            inputs: &["1", "1"],
            columns: &[(21, 2), (20, 8)],
            outputs: &[&[0, 1], &[0, 0, 0, 0, 0, 0, 0, 1]],
        },
        Case {
            sql: "SELECT $1 AS a, $2 AS b",
            oids: &[20, 21],
            inputs: &["1", "1"],
            columns: &[(20, 8), (21, 2)],
            outputs: &[&[0, 0, 0, 0, 0, 0, 0, 1], &[0, 1]],
        },
    ];
    // Act
    let evidence = run(&[cases[0], cases[1], cases[1]], true);
    // Assert
    for (evidence, case) in evidence.iter().zip([cases[0], cases[1], cases[1]]) {
        assert_case(evidence, case);
    }
    assert_eq!(
        counter(&evidence[1], "plan_cache", "misses")
            - counter(&evidence[0], "plan_cache", "misses"),
        2,
    );
    assert_eq!(
        counter(&evidence[2], "plan_cache", "misses"),
        counter(&evidence[1], "plan_cache", "misses"),
    );
    assert!(
        counter(&evidence[2], "plan_cache", "hits") > counter(&evidence[1], "plan_cache", "hits"),
    );
}

#[test]
fn should_report_parameterized_text_diagnostic_cache_hits() {
    // Arrange
    let data_type = "TEXT";
    // Act
    let hit = diagnostic_hit(data_type, "value");
    // Assert
    assert!(hit);
}

#[test]
fn should_report_contextual_bytea_diagnostic_cache_hits() {
    // Arrange
    let data_type = "BYTEA";
    // Act
    let hit = diagnostic_hit(data_type, "\\xff");
    // Assert
    assert!(hit);
}
