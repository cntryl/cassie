use std::mem::size_of;
use std::sync::Arc;

use super::validity::Validity;
use super::{check_controls, invalid, owner_bytes, QueryError};
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul, value_clone_bytes};
use crate::runtime::accounted::Accounted;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};

#[derive(Debug, Clone)]
pub(crate) struct Column(Arc<Accounted<ColumnData>>);

#[derive(Debug)]
struct ColumnData {
    data_type: DataType,
    domain: usize,
    storage: Storage,
    validity: Validity,
}

#[derive(Debug)]
enum Storage {
    Integer(Vec<i64>),
    Float(Vec<f64>),
    Boolean(Vec<u8>),
    Utf8 {
        bytes: String,
        offsets: Vec<usize>,
    },
    Scalar(Vec<Value>),
    Encoded {
        parent: Column,
        _source: Arc<Accounted<crate::midge::adapter::ValidatedEncodedField>>,
    },
    Constant(Value),
    Sequence {
        start: i64,
        step: i64,
    },
    Dictionary {
        entries: Column,
        codes: Arc<Accounted<Vec<usize>>>,
    },
    Slice {
        parent: Column,
        start: usize,
    },
    Gather {
        parent: Column,
        positions: Arc<Accounted<Vec<usize>>>,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Cell<'a> {
    Null,
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Text(&'a str),
    Scalar(&'a Value),
}

impl Cell<'_> {
    pub(crate) fn to_owned(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Integer(value) => Value::Int64(value),
            Self::Float(value) => Value::Float64(value),
            Self::Boolean(value) => Value::Bool(value),
            Self::Text(value) => Value::String(value.to_owned()),
            Self::Scalar(value) => value.clone(),
        }
    }
}

impl Column {
    pub(crate) fn from_values(
        controls: &QueryExecutionControls,
        data_type: &DataType,
        values: &[Value],
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        validate_values(data_type, values)?;
        if let Some(first) = values.first() {
            if values.iter().all(|value| same_carrier(first, value)) {
                return Self::constant(controls, data_type, first, values.len());
            }
        }
        if matches!(
            data_type,
            DataType::SmallInt | DataType::Int | DataType::BigInt
        ) && values.len() >= 2
        {
            if let (Value::Int64(start), Value::Int64(second)) = (&values[0], &values[1]) {
                if let Some(step) = second.checked_sub(*start) {
                    if values.iter().enumerate().all(|(lane, value)| {
                        let Ok(lane) = i128::try_from(lane) else { return false };
                        matches!(value, Value::Int64(value) if i128::from(*value) == i128::from(*start) + i128::from(step) * lane)
                    }) {
                        return Self::sequence(controls, data_type, *start, step, values.len());
                    }
                }
            }
        }
        if let Some(dictionary) = Self::dictionary_values(controls, data_type, values)? {
            return Ok(dictionary);
        }
        let domain = values.len();
        let payload = payload_bytes(data_type, values)?;
        let bitmap =
            if values.iter().any(Value::is_null) && values.iter().any(|value| !value.is_null()) {
                mul(domain.div_ceil(64), size_of::<u64>())?
            } else {
                0
            };
        let bytes = owner_bytes::<ColumnData>(add(
            add(payload, bitmap)?,
            data_type_clone_bytes(data_type)?,
        )?)?;
        let owner = Accounted::try_new(controls, bytes, || ColumnData {
            data_type: data_type.clone(),
            domain,
            storage: build_storage(data_type, values),
            validity: Validity::from_values(values),
        })?;
        check_controls(controls)?;
        Ok(Self(Arc::new(owner)))
    }

    pub(crate) fn len(&self) -> usize {
        self.0.get().domain
    }

    pub(crate) fn data_type(&self) -> &DataType {
        &self.0.get().data_type
    }

    pub(crate) fn cell(&self, lane: usize) -> Result<Cell<'_>, QueryError> {
        let data = self.0.get();
        if lane >= data.domain {
            return Err(invalid("column lane is outside domain"));
        }
        if !data.validity.is_valid(lane) {
            return Ok(Cell::Null);
        }
        Ok(match &data.storage {
            Storage::Integer(values) => Cell::Integer(values[lane]),
            Storage::Float(values) => Cell::Float(values[lane]),
            Storage::Boolean(values) => Cell::Boolean(values[lane] != 0),
            Storage::Utf8 { bytes, offsets } => {
                Cell::Text(&bytes[offsets[lane]..offsets[lane + 1]])
            }
            Storage::Scalar(values) => Cell::Scalar(&values[lane]),
            Storage::Encoded { parent, .. } => return parent.cell(lane),
            Storage::Constant(value) => Cell::Scalar(value),
            Storage::Sequence { start, step } => {
                let value = i128::from(*start)
                    + i128::from(*step)
                        * i128::try_from(lane)
                            .map_err(|_| invalid("sequence position overflow"))?;
                Cell::Integer(i64::try_from(value).map_err(|_| invalid("sequence value overflow"))?)
            }
            Storage::Dictionary { entries, codes } => return entries.cell(codes.get()[lane]),
            Storage::Slice { parent, start } => return parent.cell(start + lane),
            Storage::Gather { parent, positions } => return parent.cell(positions.get()[lane]),
        })
    }
}

fn validate_values(data_type: &DataType, values: &[Value]) -> Result<(), QueryError> {
    for value in values {
        let valid = match (data_type, value) {
            (DataType::SmallInt, Value::Int64(value)) => i16::try_from(*value).is_ok(),
            (DataType::Int, Value::Int64(value)) => i32::try_from(*value).is_ok(),
            (_, Value::Null)
            | (DataType::BigInt | DataType::Float, Value::Int64(_))
            | (DataType::Float, Value::Float64(_))
            | (DataType::Boolean, Value::Bool(_))
            | (
                DataType::Text | DataType::Char { .. } | DataType::Varchar { .. },
                Value::String(_),
            ) => true,
            (
                DataType::Null
                | DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Boolean
                | DataType::Text
                | DataType::Char { .. }
                | DataType::Varchar { .. },
                _,
            ) => false,
            _ => true,
        };
        if !valid {
            return Err(invalid("value does not match declared native transport"));
        }
    }
    Ok(())
}

fn payload_bytes(data_type: &DataType, values: &[Value]) -> Result<usize, crate::app::CassieError> {
    if matches!(data_type, DataType::Float)
        && values.iter().any(|value| matches!(value, Value::Int64(_)))
    {
        return mul(values.len(), size_of::<Value>());
    }
    match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => {
            mul(values.len(), size_of::<i64>())
        }
        DataType::Float => mul(values.len(), size_of::<f64>()),
        DataType::Boolean => Ok(values.len()),
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } => {
            values.iter().try_fold(
                mul(add(values.len(), 1)?, size_of::<usize>())?,
                |bytes, value| add(bytes, value.as_str().map_or(0, str::len)),
            )
        }
        _ => values
            .iter()
            .try_fold(mul(values.len(), size_of::<Value>())?, |bytes, value| {
                add(bytes, value_clone_bytes(value)?)
            }),
    }
}

fn build_storage(data_type: &DataType, values: &[Value]) -> Storage {
    if matches!(data_type, DataType::Float)
        && values.iter().any(|value| matches!(value, Value::Int64(_)))
    {
        return Storage::Scalar(values.to_vec());
    }
    match data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => Storage::Integer(
            values
                .iter()
                .map(|value| match value {
                    Value::Int64(value) => *value,
                    _ => 0,
                })
                .collect(),
        ),
        DataType::Float => Storage::Float(
            values
                .iter()
                .map(|value| match value {
                    Value::Float64(value) => *value,
                    _ => 0.0,
                })
                .collect(),
        ),
        DataType::Boolean => Storage::Boolean(
            values
                .iter()
                .map(|value| u8::from(matches!(value, Value::Bool(true))))
                .collect(),
        ),
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } => {
            let len = values.iter().filter_map(Value::as_str).map(str::len).sum();
            let mut bytes = String::with_capacity(len);
            let mut offsets = Vec::with_capacity(values.len() + 1);
            offsets.push(0);
            for value in values {
                if let Some(text) = value.as_str() {
                    bytes.push_str(text);
                }
                offsets.push(bytes.len());
            }
            Storage::Utf8 { bytes, offsets }
        }
        _ => Storage::Scalar(values.to_vec()),
    }
}

fn same_carrier(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Float64(left), Value::Float64(right)) => left.to_bits() == right.to_bits(),
        (Value::Vector(_) | Value::Json(_), _) => false,
        _ => left == right,
    }
}

impl Column {
    pub(crate) fn gather(
        &self,
        controls: &QueryExecutionControls,
        positions: &[usize],
    ) -> Result<Self, QueryError> {
        if positions.iter().any(|position| *position >= self.len()) {
            return Err(invalid("gather code is outside parent domain"));
        }
        let positions = super::shared_map(controls, positions)?;
        self.view(
            controls,
            positions.get().len(),
            Storage::Gather {
                parent: self.clone(),
                positions,
            },
        )
    }

    pub(crate) fn slice(
        &self,
        controls: &QueryExecutionControls,
        start: usize,
        len: usize,
    ) -> Result<Self, QueryError> {
        if start.checked_add(len).is_none_or(|end| end > self.len()) {
            return Err(invalid("slice is outside parent domain"));
        }
        self.view(
            controls,
            len,
            Storage::Slice {
                parent: self.clone(),
                start,
            },
        )
    }

    fn view(
        &self,
        controls: &QueryExecutionControls,
        domain: usize,
        storage: Storage,
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        let bytes = owner_bytes::<ColumnData>(data_type_clone_bytes(self.data_type())?)?;
        let owner = Accounted::try_new(controls, bytes, || ColumnData {
            data_type: self.data_type().clone(),
            domain,
            storage,
            validity: Validity::AllValid,
        })?;
        Ok(Self(Arc::new(owner)))
    }
}

impl Column {
    fn dictionary_values(
        controls: &QueryExecutionControls,
        data_type: &DataType,
        values: &[Value],
    ) -> Result<Option<Self>, QueryError> {
        if values.len() < 16
            || values
                .iter()
                .any(|value| matches!(value, Value::Json(_) | Value::Vector(_)))
        {
            return Ok(None);
        }
        let bytes = mul(
            values.len(),
            add(2 * size_of::<usize>(), size_of::<bool>())?,
        )?;
        let mut memory = controls.reserve_query_memory(bytes)?;
        let mut entries = Vec::<&Value>::with_capacity(values.len());
        let mut codes = Vec::with_capacity(values.len());
        let mut flags = Vec::with_capacity(values.len());
        for value in values {
            check_controls(controls)?;
            let code =
                if let Some(code) = entries.iter().position(|entry| same_carrier(entry, value)) {
                    code
                } else {
                    if entries.len() >= 64 {
                        return Ok(None);
                    }
                    entries.push(value);
                    entries.len() - 1
                };
            codes.push(code);
            flags.push(!value.is_null());
        }
        if entries.len() > values.len() / 4 {
            return Ok(None);
        }
        memory.try_grow(mul(entries.len(), size_of::<Value>())?)?;
        for value in &entries {
            memory.try_grow(value_clone_bytes(value)?)?;
        }
        let entries = entries
            .iter()
            .map(|value| (*value).clone())
            .collect::<Vec<_>>();
        let column = Self::from_values(controls, data_type, &entries)?;
        Ok(Some(column.dictionary(controls, &codes, &flags)?))
    }

    pub(crate) fn constant(
        controls: &QueryExecutionControls,
        data_type: &DataType,
        value: &Value,
        domain: usize,
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        validate_values(data_type, std::slice::from_ref(value))?;
        let bytes = owner_bytes::<ColumnData>(add(
            value_clone_bytes(value)?,
            data_type_clone_bytes(data_type)?,
        )?)?;
        Ok(Self(Arc::new(Accounted::try_new(controls, bytes, || {
            ColumnData {
                data_type: data_type.clone(),
                domain,
                storage: Storage::Constant(value.clone()),
                validity: if value.is_null() {
                    Validity::AllNull
                } else {
                    Validity::AllValid
                },
            }
        })?)))
    }

    pub(crate) fn sequence(
        controls: &QueryExecutionControls,
        data_type: &DataType,
        start: i64,
        step: i64,
        domain: usize,
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        if !matches!(
            data_type,
            DataType::SmallInt | DataType::Int | DataType::BigInt
        ) {
            return Err(invalid("sequence requires an integer logical type"));
        }
        if domain != 0 {
            let count =
                i128::try_from(domain - 1).map_err(|_| invalid("sequence domain overflow"))?;
            let last = i128::from(step)
                .checked_mul(count)
                .and_then(|delta| i128::from(start).checked_add(delta))
                .and_then(|value| i64::try_from(value).ok())
                .ok_or_else(|| invalid("sequence value overflow"))?;
            validate_values(data_type, &[Value::Int64(start), Value::Int64(last)])?;
        }
        Ok(Self(Arc::new(Accounted::try_new(
            controls,
            owner_bytes::<ColumnData>(0)?,
            || ColumnData {
                data_type: data_type.clone(),
                domain,
                storage: Storage::Sequence { start, step },
                validity: Validity::AllValid,
            },
        )?)))
    }

    pub(crate) fn dictionary(
        &self,
        controls: &QueryExecutionControls,
        codes: &[usize],
        validity: &[bool],
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        if codes.len() != validity.len() || codes.iter().any(|code| *code >= self.len()) {
            return Err(invalid("dictionary code or validity domain is malformed"));
        }
        let codes = super::shared_map(controls, codes)?;
        let bitmap_bytes =
            if validity.iter().all(|valid| *valid) || validity.iter().all(|valid| !*valid) {
                0
            } else {
                mul(validity.len().div_ceil(64), size_of::<u64>())?
            };
        let bytes = owner_bytes::<ColumnData>(add(
            bitmap_bytes,
            data_type_clone_bytes(self.data_type())?,
        )?)?;
        Ok(Self(Arc::new(Accounted::try_new(controls, bytes, || {
            ColumnData {
                data_type: self.data_type().clone(),
                domain: validity.len(),
                storage: Storage::Dictionary {
                    entries: self.clone(),
                    codes,
                },
                validity: Validity::from_flags(validity),
            }
        })?)))
    }
}

impl Column {
    pub(crate) fn from_encoded(
        controls: &QueryExecutionControls,
        data_type: &DataType,
        source: &Arc<Accounted<crate::midge::adapter::ValidatedEncodedField>>,
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        let field = source.get();
        if field.raw().is_empty() || field.storage_type().is_empty() {
            return Err(invalid("encoded owner lacks validated framing"));
        }
        let mut memory =
            controls.reserve_query_memory(mul(field.values().len(), size_of::<Value>())?)?;
        let mut values = Vec::with_capacity(field.values().len());
        for value in field.values() {
            check_controls(controls)?;
            memory.try_grow(crate::runtime::accounted::json::retained_bytes(value)?)?;
            values.push(crate::executor::scan::json_to_typed_value(value, data_type));
        }
        let parent = Self::from_values(controls, data_type, &values)?;
        parent.view(
            controls,
            parent.len(),
            Storage::Encoded {
                parent: parent.clone(),
                _source: Arc::clone(source),
            },
        )
    }
}
