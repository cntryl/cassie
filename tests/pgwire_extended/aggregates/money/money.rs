//! PostgreSQL 18 money result identity, locale output and overflow tracked by #908.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "../../types/money/money_integer_division_rounding.rs"]
mod money_integer_division_rounding;
#[path = "../../types/money/semantics.rs"]
mod money_residuals;

#[test]
fn should_preserve_money_type_oids_through_sum() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 12::money AS amount, SUM(1::money) AS total",
    )])];

    // Act
    let batches = run_script("postgresql18-money-locale-and-sum", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("money RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![790, 790]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("$12.00".to_owned()), Some("$1.00".to_owned())]]
    );
}

include!("errors.rs");

include!("../../types/money/money.rs");
