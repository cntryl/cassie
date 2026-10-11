//! Pending actual-helper resource and publication probes for RETURNING EXISTS.
#[path = "dml_returning_test_observer.rs"]
mod observer;
#[path = "dml_returning_test_support.rs"]
mod support;

pub(super) use observer::{observe, Phase};
use observer::{Record, Records};
use support::{assert_resource_limit, assert_result, dml_run, helper_run, projection, Fixture};

use super::{QueryError, Value};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;

fn ample_budget() -> usize {
    CassieRuntimeLimits::default().query_memory_budget_bytes
}

fn assert_success_phases(records: &Records, changed: bool) {
    let records = records.borrow();
    assert_eq!(
        records
            .iter()
            .map(|record| record.phase)
            .collect::<Vec<_>>(),
        vec![
            Phase::Entry,
            Phase::Projected,
            Phase::Materializing(0),
            Phase::Materializing(1),
        ]
    );
    let first = if changed { 11 } else { 1 };
    assert_eq!(records[0].affected, 2);
    assert_eq!(
        records[0].ids,
        vec![first, first + 1],
        "actual NEW rows at helper entry"
    );
    assert_eq!(records[1].ids, vec![first, first + 1]);
    assert!(records.iter().all(|record| record.affected == 2));
}

fn phase(records: &[Record], phase: Phase) -> &Record {
    records
        .iter()
        .find(|record| record.phase == phase)
        .expect("actual phase reached")
}

fn assert_fresh_output_admission(records: &Records, leased: bool) {
    let records = records.borrow();
    let entry = phase(&records, Phase::Entry);
    let output = phase(&records, Phase::Projected);
    assert!(entry.current > 0, "supplied statement owner was admitted");
    assert!(output.backing >= 2 * support::COPIES * support::PAYLOAD_BYTES);
    assert!(entry
        .roots
        .iter()
        .all(|roots| roots.current.is_some() == leased));
    assert!(
        output.roots.iter().all(|roots| roots.operator.is_some()),
        "actual output needs retained operator admission: {output:?}"
    );
    assert!(output.current >= entry.current + output.backing, "fresh TEXT/entry/slot backing cannot use an inherited input reservation: entry={entry:?} output={output:?}");
}

#[test]
fn should_copy_repeated_returning_text_into_distinct_fresh_buffers() {
    // Arrange
    let fixture = Fixture::new();
    let projection = projection();

    // Act
    let run = helper_run(&fixture, &projection, ample_budget(), true, false);

    // Assert
    assert_result(run.result.as_ref().expect("actual copied output"), false);
    let records = run.records.borrow();
    let entry = phase(&records, Phase::Entry);
    let projected = phase(&records, Phase::Projected);
    for id in 1..=2 {
        let source = entry
            .text
            .iter()
            .find(|text| text.id == id && text.name == "payload")
            .expect("corresponding source payload buffer");
        let copies = projected
            .text
            .iter()
            .filter(|text| text.id == id)
            .collect::<Vec<_>>();
        assert_eq!(copies.len(), support::COPIES);
        let mut addresses = std::collections::HashSet::new();
        for (index, copy) in copies.iter().enumerate() {
            assert_eq!(copy.name, format!("p{index}"));
            assert_eq!(copy.length, support::PAYLOAD_BYTES);
            assert!(copy.capacity >= copy.length);
            assert_ne!(
                copy.address, source.address,
                "actual source payload remains live while cloned"
            );
            assert!(
                addresses.insert(copy.address),
                "each repeated output has fresh backing"
            );
        }
    }
}

#[test]
fn should_admit_fresh_plain_returning_outputs_beyond_inherited_input_roots() {
    // Arrange
    let fixture = Fixture::new();
    let projection = projection();

    // Act
    let run = helper_run(&fixture, &projection, ample_budget(), true, false);

    // Assert
    assert_result(
        run.result.as_ref().expect("real leased helper result"),
        false,
    );
    assert_success_phases(&run.records, false);
    assert_fresh_output_admission(&run.records, true);
}

#[test]
fn should_admit_fresh_plain_returning_outputs_for_unleased_written_rows() {
    // Arrange
    let fixture = Fixture::new();
    let projection = projection();

    // Act
    let run = helper_run(&fixture, &projection, ample_budget(), false, false);

    // Assert
    assert_result(
        run.result.as_ref().expect("real unleased helper result"),
        false,
    );
    assert_success_phases(&run.records, false);
    assert_fresh_output_admission(&run.records, false);
}

#[test]
fn should_retain_linked_ancestor_roots_until_all_returning_values_are_public() {
    // Arrange
    let fixture = Fixture::new();
    let projection = support::ancestor_projection();

    // Act
    let run = helper_run(&fixture, &projection, ample_budget(), true, false);

    // Assert
    assert_result(
        run.result.as_ref().expect("independent public values"),
        false,
    );
    assert_success_phases(&run.records, false);
    let records = run.records.borrow();
    let entry = phase(&records, Phase::Entry);
    for roots in &entry.roots {
        assert_eq!(roots.ancestors.len(), 1, "real linked outer ancestor");
        assert!(
            !std::sync::Weak::ptr_eq(
                roots.current.as_ref().expect("current root"),
                &roots.ancestors[0]
            ),
            "distinct current and parent reservations"
        );
    }
    let output = phase(&records, Phase::Projected);
    let second = phase(&records, Phase::Materializing(1));
    assert!(
        output.entry_roots_alive,
        "projection retains actual ancestor owners: {output:?}"
    );
    assert!(
        second.entry_roots_alive && second.preceding_roots_alive,
        "row1 parents survive through row2 materialization: {second:?}"
    );
}

#[test]
fn should_admit_public_returning_slots_through_the_complete_materialization_boundary() {
    // Arrange
    let fixture = Fixture::new();
    let projection = projection();

    // Act
    let run = helper_run(&fixture, &projection, ample_budget(), true, false);

    // Assert
    assert_result(run.result.as_ref().expect("public values"), false);
    assert_success_phases(&run.records, false);
    let records = run.records.borrow();
    let projected = phase(&records, Phase::Projected);
    let slots = 2 * std::mem::size_of::<Vec<Value>>()
        + 2 * (support::COPIES + 2) * std::mem::size_of::<Value>();
    for boundary in [Phase::Materializing(0), Phase::Materializing(1)] {
        let actual = phase(&records, boundary);
        assert!(actual.current >= projected.current + slots, "public outer/Value slots need admission while all projection parents survive: projected={projected:?} boundary={actual:?} slots={slots}");
        assert!(
            actual.preceding_roots_alive,
            "earlier private parents still retained"
        );
    }
}

#[test]
fn should_enforce_measured_helper_peak_with_caller_owned_charges() {
    // Arrange
    let fixture = Fixture::new();
    let projection = projection();
    let measured = helper_run(&fixture, &projection, ample_budget(), true, true);
    let peak = measured.peak;

    // Act
    let exact = helper_run(&fixture, &projection, peak, true, true);
    let denied = helper_run(
        &fixture,
        &projection,
        peak.checked_sub(1).expect("positive peak"),
        true,
        true,
    );
    let retry = helper_run(&fixture, &projection, peak, true, true);

    // Assert
    assert!(peak < ample_budget());
    for run in [&measured, &exact, &retry] {
        assert_result(
            run.result.as_ref().expect("exact peak admits real helper"),
            false,
        );
        assert_success_phases(&run.records, false);
        assert_eq!(run.peak, peak);
        assert!(
            run.setup_peak < peak - 1,
            "input setup fits both replay caps"
        );
        assert!(run.caller_charge > 0 && run.retained >= run.caller_charge);
    }
    assert!(denied.setup_peak < peak - 1);
    assert_eq!(denied.caller_charge, measured.caller_charge);
    assert_eq!(
        denied.retained, measured.retained,
        "caller owners survive helper denial"
    );
    assert_eq!(denied.records.borrow()[0].phase, Phase::Entry);
    assert_resource_limit(&denied.result);
}

#[test]
fn should_cancel_at_actual_returning_materialization_after_successful_projection() {
    // Arrange
    let fixture = Fixture::new();
    let projection = projection();

    // Act
    let (result, records) = support::interrupt_helper(&fixture, &projection, false);

    // Assert
    assert!(records
        .borrow()
        .iter()
        .any(|record| record.phase == Phase::Projected));
    assert!(records
        .borrow()
        .iter()
        .any(|record| record.phase == Phase::Materializing(0)));
    assert!(
        matches!(result, Err(QueryError::General(ref message)) if message == "query canceled"),
        "actual final-boundary cancellation: {:?}",
        result
            .as_ref()
            .map(|result| (&result.command, result.rows.len(), result.columns.len()))
    );
    assert!(matches!(
        CassieError::from(result.expect_err("canceled helper")),
        CassieError::QueryCancelled
    ));
    fixture.assert_readback(false);
}

#[test]
fn should_expire_at_actual_returning_materialization_after_successful_projection() {
    // Arrange
    let fixture = Fixture::new();
    let projection = projection();

    // Act
    let (result, records) = support::interrupt_helper(&fixture, &projection, true);

    // Assert
    assert!(records
        .borrow()
        .iter()
        .any(|record| record.phase == Phase::Projected));
    assert!(records
        .borrow()
        .iter()
        .any(|record| record.phase == Phase::Materializing(0)));
    assert!(
        matches!(result, Err(QueryError::General(ref message)) if message == "query timeout exceeded"),
        "actual final-boundary expiry: {:?}",
        result
            .as_ref()
            .map(|result| (&result.command, result.rows.len(), result.columns.len()))
    );
    assert!(matches!(
        CassieError::from(result.expect_err("expired helper")),
        CassieError::DeadlineExceeded
    ));
    fixture.assert_readback(false);
}

#[test]
fn should_preserve_ordinary_returning_success_with_retired_private_controls() {
    // Arrange
    let fixture = Fixture::new();
    let parsed =
        crate::sql::parse_statement("SELECT id,payload FROM rt_memory").expect("ordinary AST");
    let crate::sql::ast::QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };

    // Act
    let run = helper_run(&fixture, &select.projection, ample_budget(), false, false);

    // Assert
    let result = run.result.expect("ordinary no-EXISTS control");
    assert_eq!(result.command, "UPDATE 2");
    assert_eq!(
        result.columns,
        vec![
            support::column("id", "bigint", 20, 8),
            support::column("payload", "text", 25, -1)
        ]
    );
    let mut rows = result.rows;
    rows.sort_by_key(|row| match row[0] {
        Value::Int64(id) => id,
        ref other => panic!("id: {other:?}"),
    });
    assert_eq!(
        rows,
        (1..=2)
            .map(|id| vec![
                Value::Int64(id),
                Value::String("x".repeat(support::PAYLOAD_BYTES))
            ])
            .collect::<Vec<_>>()
    );
    assert_success_phases(&run.records, false);
    fixture.assert_readback(false);
}

#[test]
fn should_abort_actual_staged_update_on_returning_attributed_peak_denial() {
    // Arrange
    let measured = dml_run(ample_budget());
    assert_result(
        measured
            .result
            .as_ref()
            .expect("ample actual changing UPDATE"),
        true,
    );
    assert_success_phases(&measured.records, true);
    let peak = measured.peak;
    let before_returning = measured.records.borrow()[0].peak;
    assert!(
        peak > before_returning && before_returning < peak - 1,
        "real RETURNING must add the measured peak: Q={before_returning} P={peak}"
    );

    // Act
    let exact = dml_run(peak);
    let denied = dml_run(peak - 1);
    let retry = dml_run(peak);

    // Assert
    for run in [&exact, &retry] {
        assert_result(run.result.as_ref().expect("fresh exact-P command"), true);
        assert_success_phases(&run.records, true);
        assert_eq!(run.peak, peak);
    }
    let records = denied.records.borrow();
    let entry = phase(&records, Phase::Entry);
    assert_eq!(entry.affected, 2);
    assert_eq!(
        entry.ids,
        vec![11, 12],
        "denial follows actual staged NEW rows"
    );
    assert!(
        entry.peak < peak - 1,
        "earlier command phases fit tight cap"
    );
    let error = denied
        .result
        .as_ref()
        .expect_err("no public result after RETURNING denial");
    assert!(
        matches!(error, CassieError::ResourceLimit(_)),
        "actual command error: {error:?}"
    );
    assert_eq!(error.descriptor().sql_state, "54000");
}
