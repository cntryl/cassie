use super::key_order_matches_value_order;
use crate::types::{DataType, DomainType};

fn domain(base: DataType) -> DataType {
    DataType::Domain(
        DomainType {
            oid: 1,
            array_oid: 2,
            qualified_name: "test.public.probe_domain".to_owned(),
            name: "probe_domain".to_owned(),
            revision: 1,
            base,
            not_null: false,
            default: None,
            checks: Vec::new(),
        }
        .into(),
    )
}

#[test]
fn should_prove_exact_index_order_only_for_integer_domain_bases() {
    // Arrange
    let exact = [DataType::SmallInt, DataType::Int, DataType::BigInt]
        .map(domain)
        .into_iter()
        .map(|data_type| key_order_matches_value_order(&data_type));
    let inexact = [
        DataType::Extended(crate::types::extended::ExtendedType::Numeric(None)),
        DataType::Date,
        DataType::Timestamp,
    ]
    .map(domain)
    .into_iter()
    .map(|data_type| key_order_matches_value_order(&data_type));

    // Act
    let exact = exact.collect::<Vec<_>>();
    let inexact = inexact.collect::<Vec<_>>();

    // Assert
    assert_eq!(exact, [true, true, true]);
    assert_eq!(inexact, [false, false, false]);
}
