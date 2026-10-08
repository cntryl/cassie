use super::*;
use std::mem::size_of;
use std::time::Instant;

#[test]
fn should_release_cte_source_construction_allowance_after_building_integer_rows() {
    // Arrange
    let limits = crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: 16 * 1024 * 1024,
        ..crate::config::CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let count = 1000;
    let relation = CteRelation {
        rows: (0..count)
            .map(|_| vec![("n".into(), Value::Int64(1))])
            .collect(),
        fields: vec![crate::types::FieldSchema {
            name: "n".into(),
            data_type: crate::types::DataType::Int,
            nullable: false,
        }],
        _rows_memory: None,
        _fields_memory: None,
    };
    // Act
    let output = copy_source_rows(&relation, &controls, Some("seq")).expect("source copy");
    let retained = controls.current_query_memory_bytes();
    let slots = output.capacity() * size_of::<BatchRow>();
    let mut actual_body = 0;
    for row in output {
        let owner = row.query_memory().expect("source owner");
        let row = row.with_query_memory(None);
        actual_body += row.unleased_body_bytes().expect("actual initial backing");
        assert_eq!(row.entries()[0].1, Value::Int64(1));
        drop(row);
        drop(owner);
    }
    // Assert
    assert!(
        retained >= slots + actual_body,
        "initial eager lookup and body must remain owned"
    );
    let handoff_allowance = count * size_of::<BatchRow>()
        + count.div_ceil(crate::executor::batch::DEFAULT_BATCH_SIZE)
            * 2
            * size_of::<super::super::Batch>()
        + retention::fields_bytes(&relation.fields).expect("field backing")
        + retention::serialization_scratch(&relation.rows).expect("serializer scratch")
        + size_of::<crate::runtime::QueryMemoryReservation>()
        + 2 * size_of::<usize>();
    assert!(
        retained <= slots + actual_body + handoff_allowance,
        "construction-only normalization/qualification charge must expire: {retained} > {}",
        slots + actual_body + handoff_allowance
    );
    assert!(controls.peak_query_memory_bytes() >= retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
