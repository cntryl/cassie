//! Finite, checked generate_series state; advancement allocates no expanded relation.
use super::{BatchRow, QueryError, SourceExecution, SourceExecutionEnv, Value};
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::FunctionCall;
use crate::sql::series_signature::{self, Kind};
use crate::types::extended::{
    numeric::Numeric,
    temporal::{self, zone::Zone, Interval},
};
use crate::types::{typed::TypedValue, DataType};
use std::cmp::Ordering;
use std::mem::size_of;
use std::sync::Arc;

enum State {
    Empty,
    Integer {
        current: Option<i64>,
        stop: i64,
        step: i64,
        int4: bool,
    },
    Decimal {
        current: Numeric,
        stop: Numeric,
        step: Numeric,
    },
    Calendar {
        current: i64,
        stop: i64,
        step: Interval,
        zone: Option<Zone>,
    },
}

pub(super) struct Cursor {
    state: State,
    data_type: DataType,
    ordinal: bool,
    position: i64,
    _memory: QueryMemoryReservation,
}

impl Cursor {
    pub(super) fn open(
        env: &SourceExecutionEnv<'_>,
        function: &FunctionCall,
        ordinal: bool,
        outer: Option<&BatchRow>,
        ctes: &super::CteContext,
    ) -> Result<Self, QueryError> {
        env.controls.check_interruption()?;
        let schema = crate::types::Schema { fields: Vec::new() };
        let signature = series_signature::resolve(function, |argument| {
            crate::sql::binder::infer_expr_type(argument, &schema, env.user_functions, &[])
        })?;
        let mut memory = env.controls.reserve_query_memory(add(
            size_of::<Self>(),
            mul(function.args.len(), size_of::<Value>())?,
        )?)?;
        let empty = BatchRow::from_projected_values(Vec::new());
        let row = outer.unwrap_or(&empty);
        let mut arguments = Vec::with_capacity(function.args.len());
        for expression in &function.args {
            env.controls.check_interruption()?;
            let _scratch =
                env.controls
                    .reserve_query_memory(super::source_set_function::argument_scratch(
                        expression, env.params, row,
                    )?)?;
            let evaluation = super::filter::EvaluationMemory::new(env.controls)?;
            let value = super::source_set_function::argument_value(
                env,
                expression,
                row,
                ctes,
                &evaluation,
            )?;
            memory.try_grow(value_clone_bytes(&value)?)?;
            arguments.push(value);
        }
        let state = if arguments.iter().any(Value::is_null) {
            State::Empty
        } else {
            Self::state(env, signature.kind, &arguments, &mut memory)?
        };
        Ok(Self {
            state,
            data_type: signature.output,
            ordinal,
            position: 0,
            _memory: memory,
        })
    }

    fn state(
        env: &SourceExecutionEnv<'_>,
        kind: Kind,
        args: &[Value],
        memory: &mut QueryMemoryReservation,
    ) -> Result<State, QueryError> {
        match kind {
            Kind::Int4 | Kind::Int8 => Self::integer_state(kind, args),
            Kind::Numeric => Self::decimal_state(args, memory),
            Kind::Timestamp | Kind::TimestampTz => Self::calendar_state(env, kind, args, memory),
        }
    }

    fn integer_state(kind: Kind, args: &[Value]) -> Result<State, QueryError> {
        let integer = |index: usize| -> Result<i64, QueryError> {
            match args.get(index) {
                Some(Value::Int64(value)) => Ok(*value),
                _ => Err(series_signature::undefined().into()),
            }
        };
        let start = integer(0)?;
        let finish = integer(1)?;
        let step = if args.len() == 3 { integer(2)? } else { 1 };
        if step == 0 {
            return Err(invalid("step size cannot equal zero"));
        }
        Ok(State::Integer {
            current: Some(start),
            stop: finish,
            step,
            int4: kind == Kind::Int4,
        })
    }

    fn decimal_state(
        args: &[Value],
        memory: &mut QueryMemoryReservation,
    ) -> Result<State, QueryError> {
        let number = |index: usize| match args.get(index) {
            Some(Value::Typed(TypedValue::Numeric(value))) => Ok(value),
            _ => Err(QueryError::from(series_signature::undefined())),
        };
        let start = number(0)?;
        let finish = number(1)?;
        let one = Numeric::from_i128(1);
        let step = if args.len() == 3 { number(2)? } else { &one };
        if !matches!(
            (start, finish, step),
            (
                Numeric::Finite { .. },
                Numeric::Finite { .. },
                Numeric::Finite { .. }
            )
        ) {
            return Err(invalid(
                "generate_series does not accept NaN or infinite numeric arguments",
            ));
        }
        if step.is_zero() {
            return Err(invalid("step size cannot equal zero"));
        }
        let scale = [start, finish, step]
            .iter()
            .filter_map(|value| value.scale())
            .max()
            .unwrap_or(0);
        let mut scratch = 1024;
        for number in [start, finish, step] {
            if let Numeric::Finite {
                mantissa,
                scale: own,
            } = number
            {
                let bits = mantissa
                    .bits()
                    .saturating_add(u64::from(scale - own) * 4)
                    .saturating_add(8);
                scratch = add(
                    scratch,
                    usize::try_from(bits.div_ceil(8)).map_err(|_| {
                        crate::app::CassieError::ResourceLimit(
                            "series numeric scratch overflow".into(),
                        )
                    })?,
                )?;
            }
        }
        memory.try_grow(mul(scratch, 16)?)?;
        Ok(State::Decimal {
            current: start.clone(),
            stop: finish.clone(),
            step: step.clone(),
        })
    }

    fn calendar_state(
        env: &SourceExecutionEnv<'_>,
        kind: Kind,
        args: &[Value],
        memory: &mut QueryMemoryReservation,
    ) -> Result<State, QueryError> {
        let session = super::filter::temporal::session_of(Some(env.controls));
        let zoned = kind == Kind::TimestampTz;
        let timestamp = |value: &Value| -> Result<i64, QueryError> {
            match value {
                Value::Typed(TypedValue::Timestamp(value) | TypedValue::TimestampTz(value)) => {
                    Ok(*value)
                }
                Value::String(text) => if zoned {
                    temporal::parse_timestamptz(text, &session, None)
                } else {
                    temporal::parse_timestamp(text, &session, None)
                }
                .map_err(type_error),
                _ => Err(series_signature::undefined().into()),
            }
        };
        let start = timestamp(&args[0])?;
        let finish = timestamp(&args[1])?;
        let step = match &args[2] {
            Value::Typed(TypedValue::Interval(value)) => *value,
            Value::String(text) => {
                temporal::parse_interval(text, -1, &session).map_err(type_error)?
            }
            _ => return Err(series_signature::undefined().into()),
        };
        if step.is_infinite() {
            return Err(invalid("step size cannot be infinite"));
        }
        if step.order_key() == 0 {
            return Err(invalid("step size cannot equal zero"));
        }
        if [start, finish]
            .iter()
            .any(|value| matches!(*value, i64::MIN | i64::MAX))
        {
            return Err(crate::app::CassieError::Unsupported(
                "infinite timestamp bounds are outside the selected finite generate_series profile"
                    .into(),
            )
            .into());
        }
        let zone = if zoned {
            if let Some(Value::String(name)) = args.get(3) {
                memory.try_grow(name.len())?;
                Some(
                    temporal::zone::lookup_zone(name)
                        .ok_or_else(|| invalid("time zone not recognized"))?,
                )
            } else {
                memory.try_grow(session.zone.name().len())?;
                Some(session.zone.clone())
            }
        } else {
            None
        };
        Ok(State::Calendar {
            current: start,
            stop: finish,
            step,
            zone,
        })
    }

    pub(super) fn next(
        &mut self,
        env: &SourceExecutionEnv<'_>,
    ) -> Result<Option<BatchRow>, QueryError> {
        env.controls.check_interruption()?;
        let heap = match &self.state {
            State::Decimal {
                current: Numeric::Finite { mantissa, .. },
                ..
            } => usize::try_from(mantissa.bits().div_ceil(8)).unwrap_or(usize::MAX),
            _ => 0,
        };
        let memory = Arc::new(env.controls.reserve_query_memory(add(1024, heap)?)?);
        let Some(value) = self.state.next()? else {
            return Ok(None);
        };
        self.position = self.position.checked_add(1).ok_or_else(|| {
            crate::app::CassieError::ResourceLimit("generate_series ordinality overflow".into())
        })?;
        let mut values = vec![(crate::sql::table_sources::physical_name(0), value)];
        let mut types = vec![self.data_type.clone()];
        if self.ordinal {
            values.push((
                crate::sql::table_sources::physical_name(1),
                Value::Int64(self.position),
            ));
            types.push(DataType::BigInt);
        }
        #[cfg(test)]
        super::source_set_function::probe::after_row();
        Ok(Some(
            BatchRow::from_projected_values(values)
                .with_optional_data_types(Some(Arc::new(types)))
                .with_query_memory(Some(memory)),
        ))
    }
}

impl State {
    fn next(&mut self) -> Result<Option<Value>, QueryError> {
        match self {
            Self::Empty => Ok(None),
            Self::Integer {
                current,
                stop,
                step,
                int4,
            } => {
                let Some(value) = *current else {
                    return Ok(None);
                };
                if (*step > 0 && value > *stop) || (*step < 0 && value < *stop) {
                    return Ok(None);
                }
                *current = value
                    .checked_add(*step)
                    .filter(|next| !*int4 || i32::try_from(*next).is_ok());
                Ok(Some(Value::Int64(value)))
            }
            Self::Decimal {
                current,
                stop,
                step,
            } => {
                let ascending = step.signum() == Some(1);
                let order = current.compare(stop);
                if (ascending && order == Ordering::Greater)
                    || (!ascending && order == Ordering::Less)
                {
                    return Ok(None);
                }
                let output = current.clone().check_format().map_err(type_error)?;
                let (
                    Numeric::Finite {
                        mantissa: left,
                        scale: left_scale,
                    },
                    Numeric::Finite {
                        mantissa: right,
                        scale: right_scale,
                    },
                ) = (&*current, &*step)
                else {
                    unreachable!("finite numeric series state")
                };
                let scale = (*left_scale).max(*right_scale);
                // The un-emitted successor may exceed the on-wire NUMERIC format.
                *current = Numeric::finite(
                    Numeric::rescaled_mantissa(left, *left_scale, scale)
                        + Numeric::rescaled_mantissa(right, *right_scale, scale),
                    scale,
                );
                Ok(Some(Value::Typed(TypedValue::Numeric(output))))
            }
            Self::Calendar {
                current,
                stop,
                step,
                zone,
            } => {
                let ascending = step.order_key() > 0;
                if (ascending && *current > *stop) || (!ascending && *current < *stop) {
                    return Ok(None);
                }
                let value = *current;
                // PostgreSQL advances before publishing even the final valid row.
                *current = temporal::ops::timestamp_plus_interval(value, step, zone.as_ref())
                    .map_err(type_error)?;
                Ok(Some(Value::Typed(if zone.is_some() {
                    TypedValue::TimestampTz(value)
                } else {
                    TypedValue::Timestamp(value)
                })))
            }
        }
    }
}

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    function: &FunctionCall,
    ordinal: bool,
    outer: Option<&BatchRow>,
    budget: Option<usize>,
    ctes: &super::CteContext,
) -> SourceExecution {
    if budget == Some(0) {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut cursor = Cursor::open(env, function, ordinal, outer, ctes)?;
    let mut rows = Vec::new();
    let mut memory = env.controls.reserve_query_memory(0)?;
    while rows.len() < budget.unwrap_or(usize::MAX) {
        let Some(row) = cursor.next(env)? else {
            break;
        };
        memory.try_grow(size_of::<BatchRow>())?;
        rows.try_reserve_exact(1).map_err(|error| {
            crate::app::CassieError::ResourceLimit(format!(
                "unable to retain series input: {error}"
            ))
        })?;
        rows.push(row);
    }
    let memory = Arc::new(memory);
    for row in &mut rows {
        row.attach_operator_memory(env.controls, Arc::clone(&memory))?;
    }
    Ok((
        super::batch::chunk_rows(rows, super::batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
    ))
}

fn invalid(message: &str) -> QueryError {
    QueryError::Cassie(crate::app::DiagnosticError::new("22023", message).into())
}
fn type_error(error: crate::types::extended::TypeInputError) -> QueryError {
    QueryError::Cassie(error.into())
}
