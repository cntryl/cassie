//! The one validator of the tagged key part, element reference and path
//! invariants. Decoding a record into the typed model is the validation, so a
//! value that decodes is valid and no other check exists to drift from it.
use uuid::Uuid;

use super::model::{Direction, ElementKind, ElementRef, KeyPart, Path};
use super::{GraphTypeKind, GRAPH_SCHEMA};
use crate::types::composite::RecordValue;
use crate::types::typed::TypedValue;
use crate::types::Value;

/// A well-formed value that violates a reserved-type invariant (`22023`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphTypeError {
    message: String,
}

impl GraphTypeError {
    fn new(kind: GraphTypeKind, detail: impl std::fmt::Display) -> Self {
        Self {
            message: format!("invalid {GRAPH_SCHEMA}.{}: {detail}", kind.type_name()),
        }
    }

    /// The diagnostic message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for GraphTypeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

type Checked<T> = Result<T, GraphTypeError>;

/// Validates `record` when it is one of the reserved types, including every
/// nested key part, element reference and array shape. Any other record is
/// accepted untouched, so callers hook this by type identity only.
///
/// # Errors
/// Returns the violated invariant of a reserved-type value.
pub fn validate_record(record: &RecordValue) -> Result<(), GraphTypeError> {
    match GraphTypeKind::of(record.descriptor()) {
        Some(GraphTypeKind::KeyPart) => KeyPart::from_record(record).map(drop),
        Some(GraphTypeKind::ElementRef) => ElementRef::from_record(record).map(drop),
        Some(GraphTypeKind::Path) => Path::from_record(record).map(drop),
        None => Ok(()),
    }
}

fn expect_kind(record: &RecordValue, kind: GraphTypeKind) -> Checked<()> {
    if GraphTypeKind::of(record.descriptor()) == Some(kind)
        && record.fields().len() == field_count(kind)
    {
        Ok(())
    } else {
        Err(GraphTypeError::new(kind, "the record is not of this type"))
    }
}

const fn field_count(kind: GraphTypeKind) -> usize {
    match kind {
        GraphTypeKind::KeyPart => 4,
        GraphTypeKind::ElementRef => 5,
        GraphTypeKind::Path => 3,
    }
}

fn required<'a>(
    kind: GraphTypeKind,
    record: &'a RecordValue,
    index: usize,
    field: &str,
) -> Checked<&'a Value> {
    match record.fields().get(index) {
        Some(Value::Null) | None => Err(GraphTypeError::new(
            kind,
            format!("field {field} must not be NULL"),
        )),
        Some(value) => Ok(value),
    }
}

fn small_tag(kind: GraphTypeKind, value: &Value, field: &str) -> Checked<i16> {
    match value {
        Value::Int64(number) => i16::try_from(*number)
            .map_err(|_| GraphTypeError::new(kind, format!("field {field} is out of range"))),
        _ => Err(GraphTypeError::new(
            kind,
            format!("field {field} must be a smallint"),
        )),
    }
}

fn parse_uuid(kind: GraphTypeKind, value: &Value, field: &str) -> Checked<Uuid> {
    match value {
        Value::String(text) => crate::types::uuid_text::parse(text),
        _ => None,
    }
    .ok_or_else(|| GraphTypeError::new(kind, format!("field {field} must be a uuid")))
}

impl KeyPart {
    /// Decodes and validates a `graph_key_part` record.
    ///
    /// # Errors
    /// Returns the violated invariant: an unknown tag, a NULL tag, a missing or
    /// extra populated slot, or an integer outside the tagged type's range.
    pub fn from_record(record: &RecordValue) -> Checked<Self> {
        const KIND: GraphTypeKind = GraphTypeKind::KeyPart;
        expect_kind(record, KIND)?;
        let tag = small_tag(KIND, required(KIND, record, 0, "type_tag")?, "type_tag")?;
        let fields = record.fields();
        let populated = fields[1..].iter().filter(|value| !value.is_null()).count();
        if populated != 1 {
            return Err(GraphTypeError::new(
                KIND,
                format!("type_tag {tag} requires exactly one populated value slot"),
            ));
        }
        let wrong_slot = || {
            GraphTypeError::new(
                KIND,
                format!("type_tag {tag} populates the wrong value slot"),
            )
        };
        let integer = |bounded: fn(i64) -> Option<Self>| match &fields[1] {
            Value::Int64(number) => bounded(*number).ok_or_else(|| {
                GraphTypeError::new(
                    KIND,
                    format!("integer_value {number} is out of range for type_tag {tag}"),
                )
            }),
            _ => Err(wrong_slot()),
        };
        match tag {
            1 => integer(|n| i16::try_from(n).ok().map(Self::SmallInt)),
            2 => integer(|n| i32::try_from(n).ok().map(Self::Int)),
            3 => integer(|n| Some(Self::BigInt(n))),
            4 => match &fields[2] {
                Value::String(text) => Ok(Self::Text(text.clone())),
                _ => Err(wrong_slot()),
            },
            5 if fields[3].is_null() => Err(wrong_slot()),
            5 => parse_uuid(KIND, &fields[3], "uuid_value").map(Self::Uuid),
            _ => Err(GraphTypeError::new(KIND, format!("unknown type_tag {tag}"))),
        }
    }
}

/// The elements of a record array field: one-dimensional with lower bound 1,
/// or the canonical empty array; every element a non-NULL record of `element`.
fn record_array<'a>(
    kind: GraphTypeKind,
    value: &'a Value,
    field: &str,
    element: GraphTypeKind,
) -> Checked<Vec<&'a RecordValue>> {
    let elements = shaped_array(kind, value, field)?;
    elements
        .iter()
        .map(|item| match item {
            Value::Typed(TypedValue::Record(record))
                if GraphTypeKind::of(record.descriptor()) == Some(element) =>
            {
                Ok(record)
            }
            Value::Null => Err(GraphTypeError::new(
                kind,
                format!("array {field} must not contain NULL elements"),
            )),
            _ => Err(GraphTypeError::new(
                kind,
                format!(
                    "array {field} must contain only {} values",
                    element.type_name()
                ),
            )),
        })
        .collect()
}

fn shaped_array<'a>(kind: GraphTypeKind, value: &'a Value, field: &str) -> Checked<&'a [Value]> {
    let Value::Array(array) = value else {
        return Err(GraphTypeError::new(
            kind,
            format!("field {field} must be an array"),
        ));
    };
    let dimensions = array.dimensions();
    let canonical = if array.elements().is_empty() {
        dimensions.is_empty()
    } else {
        matches!(dimensions, [dimension]
            if dimension.lower_bound == 1
                && usize::try_from(dimension.length).ok() == Some(array.elements().len()))
    };
    if canonical {
        Ok(array.elements())
    } else {
        Err(GraphTypeError::new(
            kind,
            format!("array {field} must be one-dimensional with lower bound 1"),
        ))
    }
}

impl ElementRef {
    /// Decodes and validates a `graph_element_ref` record, including its key.
    ///
    /// # Errors
    /// Returns the violated invariant: a NULL mandatory field, an unknown
    /// kind, an empty key or an invalid key part or array shape.
    pub fn from_record(record: &RecordValue) -> Checked<Self> {
        const KIND: GraphTypeKind = GraphTypeKind::ElementRef;
        expect_kind(record, KIND)?;
        let identity = |index: usize, field: &str| {
            parse_uuid(KIND, required(KIND, record, index, field)?, field)
        };
        let database_id = identity(0, "database_id")?;
        let graph_id = identity(1, "graph_id")?;
        let mapping_id = identity(2, "mapping_id")?;
        let kind = match small_tag(
            KIND,
            required(KIND, record, 3, "element_kind")?,
            "element_kind",
        )? {
            1 => ElementKind::Vertex,
            2 => ElementKind::Edge,
            other => {
                return Err(GraphTypeError::new(
                    KIND,
                    format!("unknown element_kind {other}"),
                ))
            }
        };
        let parts = record_array(
            KIND,
            required(KIND, record, 4, "key_parts")?,
            "key_parts",
            GraphTypeKind::KeyPart,
        )?;
        if parts.is_empty() {
            return Err(GraphTypeError::new(KIND, "key_parts must not be empty"));
        }
        let key = parts
            .into_iter()
            .map(KeyPart::from_record)
            .collect::<Checked<Vec<_>>>()?;
        Ok(Self {
            database_id,
            graph_id,
            mapping_id,
            kind,
            key,
        })
    }
}

impl Path {
    /// Decodes and validates a `graph_path` record, including every element.
    ///
    /// # Errors
    /// Returns the violated invariant: a NULL field, a wrong element kind,
    /// a vertex count other than edge count plus one, a direction count other
    /// than the edge count or a direction outside 1 and 2.
    pub fn from_record(record: &RecordValue) -> Checked<Self> {
        const KIND: GraphTypeKind = GraphTypeKind::Path;
        expect_kind(record, KIND)?;
        let elements = |index: usize, field: &str, expected: ElementKind| {
            record_array(
                KIND,
                required(KIND, record, index, field)?,
                field,
                GraphTypeKind::ElementRef,
            )?
            .into_iter()
            .map(|element| {
                let reference = ElementRef::from_record(element)?;
                if reference.kind == expected {
                    Ok(reference)
                } else {
                    Err(GraphTypeError::new(
                        KIND,
                        format!("every element of {field} must be a {}", expected.name()),
                    ))
                }
            })
            .collect::<Checked<Vec<_>>>()
        };
        let vertices = elements(0, "vertices", ElementKind::Vertex)?;
        let edges = elements(1, "edges", ElementKind::Edge)?;
        let directions =
            shaped_array(KIND, required(KIND, record, 2, "directions")?, "directions")?
                .iter()
                .map(|value| match value {
                    Value::Int64(1) => Ok(Direction::Forward),
                    Value::Int64(2) => Ok(Direction::Reverse),
                    Value::Null => Err(GraphTypeError::new(
                        KIND,
                        "array directions must not contain NULL elements",
                    )),
                    _ => Err(GraphTypeError::new(KIND, "direction must be 1 or 2")),
                })
                .collect::<Checked<Vec<_>>>()?;
        if vertices.len() != edges.len() + 1 {
            return Err(GraphTypeError::new(
                KIND,
                "vertex count must equal edge count plus one",
            ));
        }
        if directions.len() != edges.len() {
            return Err(GraphTypeError::new(
                KIND,
                "direction count must equal edge count",
            ));
        }
        Ok(Self {
            vertices,
            edges,
            directions,
        })
    }
}
