//! Typed model of the three reserved composite types, with the encoders that
//! build their record values.
use uuid::Uuid;

use crate::types::composite::{CompositeRef, RecordValue};
use crate::types::typed::TypedValue;
use crate::types::{SqlArray, Value};

/// One typed key component. Variant order is the contract's type order:
/// smallint, integer, bigint, text, uuid.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyPart {
    SmallInt(i16),
    Int(i32),
    BigInt(i64),
    /// Text with C collation: ordered by bytes.
    Text(String),
    /// Ordered as unsigned 16-byte values.
    Uuid(Uuid),
}

impl KeyPart {
    /// The `type_tag` discriminator (1 to 5).
    #[must_use]
    pub const fn type_tag(&self) -> i16 {
        match self {
            Self::SmallInt(_) => 1,
            Self::Int(_) => 2,
            Self::BigInt(_) => 3,
            Self::Text(_) => 4,
            Self::Uuid(_) => 5,
        }
    }

    /// The SQL type name used by the JSON export.
    #[must_use]
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::SmallInt(_) => "smallint",
            Self::Int(_) => "integer",
            Self::BigInt(_) => "bigint",
            Self::Text(_) => "text",
            Self::Uuid(_) => "uuid",
        }
    }

    /// The canonical decimal, text or lowercase UUID form of the value.
    #[must_use]
    pub fn value_text(&self) -> String {
        match self {
            Self::SmallInt(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::BigInt(value) => value.to_string(),
            Self::Text(value) => value.clone(),
            Self::Uuid(value) => value.to_string(),
        }
    }

    /// The four slot values `(integer_value, text_value, uuid_value)`.
    fn slots(&self) -> [Value; 3] {
        match self {
            Self::SmallInt(value) => [Value::Int64(i64::from(*value)), Value::Null, Value::Null],
            Self::Int(value) => [Value::Int64(i64::from(*value)), Value::Null, Value::Null],
            Self::BigInt(value) => [Value::Int64(*value), Value::Null, Value::Null],
            Self::Text(value) => [Value::Null, Value::String(value.clone()), Value::Null],
            Self::Uuid(value) => [Value::Null, Value::Null, Value::String(value.to_string())],
        }
    }
}

/// Whether an element is a vertex (1) or an edge (2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ElementKind {
    Vertex,
    Edge,
}

impl ElementKind {
    /// The `element_kind` discriminator.
    #[must_use]
    pub const fn tag(self) -> i16 {
        match self {
            Self::Vertex => 1,
            Self::Edge => 2,
        }
    }

    /// The JSON export name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::Edge => "edge",
        }
    }
}

/// Traversal direction of one path edge: forward before reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    Forward,
    Reverse,
}

impl Direction {
    /// The stored `directions` element (1 or 2).
    #[must_use]
    pub const fn tag(self) -> i16 {
        match self {
            Self::Forward => 1,
            Self::Reverse => 2,
        }
    }
}

/// A durable element reference: mapping plus typed key, with no row incarnation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ElementRef {
    pub database_id: Uuid,
    pub graph_id: Uuid,
    pub mapping_id: Uuid,
    pub kind: ElementKind,
    pub key: Vec<KeyPart>,
}

/// A detached structural path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Path {
    pub vertices: Vec<ElementRef>,
    pub edges: Vec<ElementRef>,
    pub directions: Vec<Direction>,
}

/// The descriptors of the three reserved types of one database.
#[derive(Debug, Clone)]
pub struct GraphDescriptors {
    pub key_part: CompositeRef,
    pub element_ref: CompositeRef,
    pub path: CompositeRef,
}

fn record(descriptor: &CompositeRef, fields: Vec<Value>) -> Result<Value, String> {
    RecordValue::new(descriptor.clone(), fields).map(|record| Value::Typed(TypedValue::Record(record)))
}

fn array(elements: Vec<Value>) -> Result<Value, String> {
    SqlArray::from_elements(elements).map(Value::Array)
}

impl KeyPart {
    /// The `graph_key_part` record of this component.
    ///
    /// # Errors
    /// Returns a message when `descriptors` does not describe the type.
    pub fn to_value(&self, descriptors: &GraphDescriptors) -> Result<Value, String> {
        let [integer, text, uuid] = self.slots();
        record(
            &descriptors.key_part,
            vec![Value::Int64(i64::from(self.type_tag())), integer, text, uuid],
        )
    }
}

impl ElementRef {
    /// The `graph_element_ref` record of this reference.
    ///
    /// # Errors
    /// Returns a message when `descriptors` does not describe the types.
    pub fn to_value(&self, descriptors: &GraphDescriptors) -> Result<Value, String> {
        let parts = self
            .key
            .iter()
            .map(|part| part.to_value(descriptors))
            .collect::<Result<Vec<_>, _>>()?;
        record(
            &descriptors.element_ref,
            vec![
                Value::String(self.database_id.to_string()),
                Value::String(self.graph_id.to_string()),
                Value::String(self.mapping_id.to_string()),
                Value::Int64(i64::from(self.kind.tag())),
                array(parts)?,
            ],
        )
    }
}

impl Path {
    /// The `graph_path` record of this path.
    ///
    /// # Errors
    /// Returns a message when `descriptors` does not describe the types.
    pub fn to_value(&self, descriptors: &GraphDescriptors) -> Result<Value, String> {
        let references = |elements: &[ElementRef]| {
            elements
                .iter()
                .map(|element| element.to_value(descriptors))
                .collect::<Result<Vec<_>, _>>()
                .and_then(array)
        };
        let directions = self
            .directions
            .iter()
            .map(|direction| Value::Int64(i64::from(direction.tag())))
            .collect();
        record(
            &descriptors.path,
            vec![
                references(&self.vertices)?,
                references(&self.edges)?,
                array(directions)?,
            ],
        )
    }
}
