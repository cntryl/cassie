use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, hash_table_bytes, mul};
use crate::runtime::QueryExecutionControls;
use crate::types::DataType;

/// Heap backing in addition to a value's inline slot. Source capacities bound
/// the corresponding clone, including sparse containers and nested buffers.
pub(super) trait HeapBytes {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError>;
}

macro_rules! copy_heap {
    ($($kind:ty),* $(,)?) => {$ (
        impl HeapBytes for $kind {
            fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
                fn require_copy<T: Copy>(_: &T) {}
                require_copy(self);
                controls.check_interruption()?;
                Ok(0)
            }
        }
    )*};
}

copy_heap!(
    bool,
    u8,
    u16,
    u32,
    u64,
    i64,
    usize,
    f32,
    f64,
    crate::catalog::DefaultSequenceOwnership,
    crate::types::ObjectIdKind,
    crate::types::extended::ExtendedType,
);

impl HeapBytes for String {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        Ok(self.capacity())
    }
}

impl<T: HeapBytes> HeapBytes for Option<T> {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        self.as_ref()
            .map_or(Ok(0), |value| value.heap_bytes(controls))
    }
}

impl<T: HeapBytes> HeapBytes for Box<T> {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        add(size_of::<T>(), self.as_ref().heap_bytes(controls)?)
    }
}

impl<T: HeapBytes> HeapBytes for Vec<T> {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        self.iter()
            .try_fold(mul(self.capacity(), size_of::<T>())?, |bytes, item| {
                add(bytes, item.heap_bytes(controls)?)
            })
    }
}

impl<K: HeapBytes, V: HeapBytes> HeapBytes for HashMap<K, V> {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        self.iter().try_fold(
            hash_table_bytes::<(K, V)>(self.capacity())?,
            |bytes, (key, value)| {
                add(
                    add(bytes, key.heap_bytes(controls)?)?,
                    value.heap_bytes(controls)?,
                )
            },
        )
    }
}

// This is the existing retained JSON BTree bound generalized to typed maps:
// eleven occupied/unused slots and sixteen pointer words per entry. An empty
// map may retain a root after deletion. Requalify with Rust's BTree layout.
fn tree_backing<K, V>(entries: usize) -> Result<usize, CassieError> {
    mul(
        entries.max(1),
        add(
            mul(11, add(size_of::<K>(), size_of::<V>())?)?,
            16 * size_of::<usize>(),
        )?,
    )
}

impl<K: HeapBytes, V: HeapBytes> HeapBytes for BTreeMap<K, V> {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        self.iter()
            .try_fold(tree_backing::<K, V>(self.len())?, |bytes, (key, value)| {
                add(
                    add(bytes, key.heap_bytes(controls)?)?,
                    value.heap_bytes(controls)?,
                )
            })
    }
}

impl<T: HeapBytes> HeapBytes for BTreeSet<T> {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        self.iter()
            .try_fold(tree_backing::<T, ()>(self.len())?, |bytes, value| {
                add(bytes, value.heap_bytes(controls)?)
            })
    }
}

impl HeapBytes for serde_json::Value {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        match self {
            Self::String(value) => value.heap_bytes(controls),
            Self::Array(values) => values.heap_bytes(controls),
            Self::Object(values) => values.iter().try_fold(
                mul(
                    values.len().max(1),
                    crate::runtime::accounted::json::JSON_MAP_ENTRY_BYTES,
                )?,
                |bytes, (key, value)| {
                    add(
                        add(bytes, key.heap_bytes(controls)?)?,
                        value.heap_bytes(controls)?,
                    )
                },
            ),
            Self::Number(value) => add(mul(value.as_str().len(), 2)?, 64),
            Self::Null | Self::Bool(_) => Ok(0),
        }
    }
}

impl HeapBytes for DataType {
    fn heap_bytes(&self, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        match self {
            Self::Array(inner) => inner.heap_bytes(controls),
            Self::Char { length } | Self::Varchar { length } => length.heap_bytes(controls),
            Self::Vector(width) => width.heap_bytes(controls),
            Self::ObjectId(kind) => kind.heap_bytes(controls),
            Self::Extended(kind) => kind.heap_bytes(controls),
            Self::Null
            | Self::SmallInt
            | Self::Int
            | Self::BigInt
            | Self::Float
            | Self::Boolean
            | Self::Name
            | Self::Text
            | Self::Uuid
            | Self::Bytea
            | Self::Date
            | Self::Time
            | Self::Timestamp
            | Self::Json
            | Self::Jsonb
            | Self::JsonPath => Ok(0),
        }
    }
}
