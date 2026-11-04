//! Admit both native text renderings before result publication allocates strings.
use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{
    extended::{numeric::Numeric, temporal::TemporalSession},
    typed::TypedValue,
    Value,
};

pub(super) fn context_bytes(session: &TemporalSession) -> Result<usize, CassieError> {
    let name = match &session.zone {
        crate::types::extended::temporal::zone::Zone::Fixed { name, .. }
        | crate::types::extended::temporal::zone::Zone::Rules { name, .. } => name.capacity(),
    };
    add(
        std::mem::size_of::<TemporalSession>() + 2 * std::mem::size_of::<usize>(),
        name,
    )
}

pub(super) fn admit<'a>(
    mut values: impl Iterator<Item = &'a Value>,
    controls: &QueryExecutionControls,
    session: &TemporalSession,
) -> Result<QueryMemoryReservation, CassieError> {
    let bytes = values.try_fold(0, |bytes, value| {
        controls.check_interruption()?;
        let Value::Typed(value) = value else {
            return Ok(bytes);
        };
        add(bytes, mul(bound(value, session)?.max(128), 4)?)
    })?;
    controls.reserve_query_memory(bytes)
}

pub(crate) fn bound(value: &TypedValue, session: &TemporalSession) -> Result<usize, CassieError> {
    match value {
        TypedValue::Numeric(Numeric::Finite { mantissa, scale }) => {
            // log10(2) < 1: binary digit count bounds decimal magnitude without
            // allocating the decimal digits. Scale covers leading fractional zeros.
            let bits = usize::try_from(mantissa.bits()).map_err(|_| {
                CassieError::ResourceLimit("numeric rendering bound overflow".into())
            })?;
            add(
                add(
                    bits,
                    usize::try_from(*scale).map_err(|_| {
                        CassieError::ResourceLimit("numeric scale bound overflow".into())
                    })?,
                )?,
                16,
            )
        }
        TypedValue::Bit(value) | TypedValue::VarBit(value) => add(mul(value.heap_bytes(), 8)?, 128),
        TypedValue::TimestampTz(_) => add(session.zone.name().len(), 256),
        TypedValue::Real(_)
        | TypedValue::Numeric(_)
        | TypedValue::Inet(_)
        | TypedValue::Cidr(_)
        | TypedValue::Macaddr(_)
        | TypedValue::Macaddr8(_)
        | TypedValue::PgLsn(_)
        | TypedValue::Date(_)
        | TypedValue::Time(_)
        | TypedValue::TimeTz(_)
        | TypedValue::Timestamp(_)
        | TypedValue::Interval(_) => Ok(128),
    }
}

pub(super) fn format(value: &TypedValue, session: &TemporalSession) -> String {
    #[cfg(test)]
    RENDER_COUNT.with(|count| count.set(count.get() + 1));
    value.text_in(session)
}

#[cfg(test)]
thread_local! { static RENDER_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::QueryResult;
    use std::time::Instant;

    #[test]
    fn should_admit_native_numeric_rendering_before_either_result_text_allocation() {
        // Arrange
        let value = Value::Typed(TypedValue::Numeric(Numeric::finite(
            num_bigint::BigInt::from(1),
            4_096,
        )));

        for (budget, accepted, expected_renders) in [(4_096, false, 0), (32_768, true, 2)] {
            // Arrange
            let controls = QueryExecutionControls::from_limits(
                &crate::config::CassieRuntimeLimits {
                    query_memory_budget_bytes: budget,
                    ..crate::config::CassieRuntimeLimits::default()
                },
                Instant::now(),
            );
            RENDER_COUNT.with(|count| count.set(0));
            let mut builder = QueryResult::try_builder(&controls).expect("result owner");
            // Act
            let result = builder.try_push_row(std::slice::from_ref(&value));
            // Assert
            assert_eq!(result.is_ok(), accepted);
            if !accepted {
                assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
            }
            assert_eq!(RENDER_COUNT.with(std::cell::Cell::get), expected_renders);
            drop(builder);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}
