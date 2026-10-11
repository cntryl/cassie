use std::cell::Cell;
use std::fmt;
use std::rc::Rc;
use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;

use super::PlanAllocation;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1024,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

struct ChangingDisplay<'a> {
    calls: Cell<usize>,
    large: &'a str,
}

impl fmt::Display for ChangingDisplay<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let call = self.calls.get();
        self.calls.set(call + 1);
        formatter.write_str(if call == 0 { "x" } else { self.large })
    }
}

#[test]
fn should_reject_explain_format_growth_before_unadmitted_copy() {
    // Arrange
    let controls = controls();
    let large = "x".repeat(8192);
    let value = ChangingDisplay { calls: Cell::new(0), large: &large };
    let mut allocation = PlanAllocation::new(&controls, 0).expect("empty admission");

    // Act
    let output = allocation.format(format_args!("{value}"));
    let rejected = matches!(output, Err(CassieError::ResourceLimit(_)));
    let calls = value.calls.get();
    drop(output);
    drop(allocation);
    let released = controls.current_query_memory_bytes();
    let peak = controls.peak_query_memory_bytes();

    // Assert
    assert!(rejected, "second-pass output must not grow beyond its counted admission");
    assert_eq!(calls, 2);
    assert_eq!(peak, 1);
    assert_eq!(released, 0);
}

#[derive(Clone)]
struct ChangingItems {
    calls: Rc<Cell<usize>>,
}

impl IntoIterator for ChangingItems {
    type Item = &'static str;
    type IntoIter = std::iter::Take<std::array::IntoIter<&'static str, 2>>;

    fn into_iter(self) -> Self::IntoIter {
        let call = self.calls.get();
        self.calls.set(call + 1);
        ["", ""].into_iter().take(if call == 0 { 1 } else { 2 })
    }
}

#[test]
fn should_reject_explain_separator_growth_before_unadmitted_copy() {
    // Arrange
    let controls = controls();
    let separator = "x".repeat(8192);
    let calls = Rc::new(Cell::new(0));
    let values = ChangingItems { calls: Rc::clone(&calls) };
    let mut allocation = PlanAllocation::new(&controls, 0).expect("empty admission");

    // Act
    let output = allocation.join(values, &separator);
    let rejected = matches!(output, Err(CassieError::ResourceLimit(_)));
    drop(output);
    drop(allocation);
    let released = controls.current_query_memory_bytes();
    let peak = controls.peak_query_memory_bytes();

    // Assert
    assert!(rejected, "separator writes must obey the same capacity guard as value writes");
    assert_eq!(calls.get(), 2);
    assert_eq!(peak, 0);
    assert_eq!(released, 0);
}
