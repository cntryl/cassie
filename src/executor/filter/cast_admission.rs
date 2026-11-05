//! Admission for reached casts that can expand a compact scalar input.
use super::{DataType, EvalContext, QueryError, ScalarValue, Value};
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::types::{extended::numeric, typed::TypedValue};

pub(super) fn target(target: &DataType, context: EvalContext<'_>) -> Result<(), QueryError> {
    if let (
        Some(memory),
        DataType::Char {
            length: Some(width),
        },
    ) = (context.memory, target)
    {
        // Two UTF-8 buffers: the cast result and ScalarValue -> Value copy.
        memory.grow(mul(
            usize::try_from(*width)
                .map_err(|_| QueryError::General("CHAR width overflow".into()))?,
            8,
        )?)?;
    }
    Ok(())
}

pub(super) fn value(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
    context: EvalContext<'_>,
) -> Result<(), QueryError> {
    let Some(memory) = context.memory else {
        return Ok(());
    };
    let bytes = if *target == DataType::Jsonb || source == Some(&DataType::Jsonb) {
        match value {
            ScalarValue::Str(text) | ScalarValue::Json(text) => {
                crate::types::jsonb::input_text_len(text)
                    .map_err(crate::app::CassieError::TypeInput)?
            }
            _ => 0,
        }
    } else if matches!(
        target,
        DataType::Extended(crate::types::extended::ExtendedType::Numeric(_))
    ) {
        match value {
            ScalarValue::Str(text) => numeric_input_bytes(text)?,
            _ => 0,
        }
    } else {
        0
    };
    // Parser trees plus canonical output and evaluator/result copies remain
    // owned until the enclosing expression (including native matching) ends.
    memory.grow(mul(bytes, 8)?)
}

/// A conservative, allocation-free bound for numeric text expansion. Input
/// grammar and range errors remain owned by `numeric::parse_text`. Decimal
/// exponents (including separators) can add digits before or after the point;
/// radix literals expand by at most four bits per source digit instead.
pub(super) fn numeric_input_bytes(text: &str) -> Result<usize, QueryError> {
    let body = text.trim().trim_start_matches(['+', '-']);
    let radix = body.get(..2).is_some_and(|prefix| {
        ["0x", "0o", "0b"]
            .iter()
            .any(|radix| prefix.eq_ignore_ascii_case(radix))
    });
    let exponent = if radix {
        0
    } else {
        body.rsplit_once(['e', 'E'])
            .and_then(|(_, raw)| {
                raw.trim_start_matches(['+', '-'])
                    .bytes()
                    .filter(|byte| *byte != b'_')
                    .try_fold(0usize, |value, byte| {
                        if !byte.is_ascii_digit() {
                            return None;
                        }
                        value.checked_mul(10)?.checked_add(usize::from(byte - b'0'))
                    })
            })
            .unwrap_or(0)
    };
    // Larger nonzero values fail the existing shape check before coefficient
    // construction. Zero coefficients need only their bounded display scale.
    let expansion = exponent.min(numeric::MAX_INTEGER_DIGITS + numeric::MAX_DSCALE as usize);
    Ok(add(mul(text.len(), 4)?, add(expansion, 16)?)?)
}

pub(super) fn value_workspace_bytes(value: &Value) -> Result<usize, QueryError> {
    let bytes = value_clone_bytes(value)?;
    let formatted = match value {
        Value::Typed(TypedValue::Numeric(numeric::Numeric::Finite { mantissa, scale })) => {
            // Binary bit count bounds decimal digits; display-scale padding is
            // independent of the retained coefficient's allocation.
            add(
                usize::try_from(mantissa.bits())
                    .map_err(|_| QueryError::General("numeric size overflow".into()))?,
                *scale as usize + 3,
            )?
        }
        Value::Typed(TypedValue::Bit(bits) | TypedValue::VarBit(bits)) => bits.len(),
        Value::Array(array) => array
            .elements()
            .iter()
            .try_fold(0, |total, value| add(total, value_workspace_bytes(value)?))?,
        _ => 0,
    };
    Ok(add(bytes, formatted)?)
}
