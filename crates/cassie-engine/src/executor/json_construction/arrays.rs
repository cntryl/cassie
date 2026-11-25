//! SQL-to-JSON array construction without the general storage JSON carrier.
use super::{check, DataType, QueryError, QueryExecutionControls};
use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};
use crate::types::SqlArray;

fn element_type(mut kind: Option<&DataType>) -> Option<&DataType> {
    while let Some(DataType::Array(element)) = kind {
        kind = Some(element);
    }
    kind
}

/// The caller reserves this bound through the existing construction workspace
/// before encoding. One scalar fragment and the destination coexist under it.
pub(super) fn bound(
    array: &SqlArray,
    kind: Option<&DataType>,
    binary: bool,
) -> Result<usize, CassieError> {
    bound_controlled(array, kind, binary, None)
}

pub(super) fn bound_controlled(
    array: &SqlArray,
    kind: Option<&DataType>,
    binary: bool,
    controls: Option<&QueryExecutionControls>,
) -> Result<usize, CassieError> {
    let kind = element_type(kind);
    array
        .elements()
        .iter()
        .try_fold(shape_bytes(array)?, |bytes, value| {
            if let Some(controls) = controls {
                controls.check_interruption()?;
            }
            let canonical = super::bound_controlled(value, kind, binary, controls)?;
            // Canonical JSONB can be much shorter than an exact Number's original
            // exponent spelling. Its raw fragment is allocated before canonicalization.
            let raw = if binary {
                super::bound_controlled(value, kind, false, controls)?
            } else {
                canonical
            };
            add(bytes, canonical.max(raw))
        })
}

fn shape_bytes(array: &SqlArray) -> Result<usize, CassieError> {
    let dimensions = array.dimensions();
    if dimensions.len() > 6 {
        return Err(crate::app::DiagnosticError::new(
            "54000",
            "number of array dimensions exceeds the maximum allowed (6)",
        )
        .into());
    }
    let mut containers = usize::from(!dimensions.is_empty());
    let mut bytes = 0;
    for dimension in dimensions {
        let length = usize::try_from(dimension.length)
            .ok()
            .filter(|length| *length != 0)
            .ok_or_else(|| CassieError::ArrayInput("invalid native array dimension".into()))?;
        bytes = add(bytes, mul(containers, add(2, length - 1)?)?)?;
        containers = mul(containers, length)?;
    }
    if containers != array.elements().len() {
        return Err(CassieError::ArrayInput(
            "native array shape does not match its elements".into(),
        ));
    }
    Ok(bytes.max(2))
}

pub(super) fn encode(
    array: &SqlArray,
    kind: Option<&DataType>,
    controls: Option<&QueryExecutionControls>,
) -> Result<String, QueryError> {
    encode_observing(array, kind, controls, || {})
}

/// The private callback runs after an actual scalar fragment has been encoded.
/// It carries no request-independent hook or production configuration.
pub(super) fn encode_observing(
    array: &SqlArray,
    kind: Option<&DataType>,
    controls: Option<&QueryExecutionControls>,
    mut after_element: impl FnMut(),
) -> Result<String, QueryError> {
    check(controls)?;
    let capacity = bound_controlled(array, kind, false, controls)?;
    let mut output = String::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if output.capacity() > capacity {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted JSON array text capacity".into(),
        )
        .into());
    }
    let mut encoder = Encoder {
        array,
        kind: element_type(kind),
        controls,
        output,
        cursor: 0,
        after_element: &mut after_element,
    };
    encoder.dimension(0)?;
    check(controls)?;
    Ok(encoder.output)
}

struct Encoder<'a, 'b, F> {
    array: &'a SqlArray,
    kind: Option<&'a DataType>,
    controls: Option<&'a QueryExecutionControls>,
    output: String,
    cursor: usize,
    after_element: &'b mut F,
}

impl<F: FnMut()> Encoder<'_, '_, F> {
    fn dimension(&mut self, depth: usize) -> Result<(), QueryError> {
        check(self.controls)?;
        self.output.push('[');
        if let Some(dimension) = self.array.dimensions().get(depth) {
            for index in 0..dimension.length {
                check(self.controls)?;
                if index != 0 {
                    self.output.push(',');
                }
                if depth + 1 == self.array.dimensions().len() {
                    let fragment = super::encode(
                        &self.array.elements()[self.cursor],
                        self.kind,
                        self.controls,
                    )?;
                    (self.after_element)();
                    check(self.controls)?;
                    if fragment.len() > self.output.capacity() - self.output.len() {
                        return Err(CassieError::ResourceLimit(
                            "JSON array text exceeded its admitted bound".into(),
                        )
                        .into());
                    }
                    self.output.push_str(&fragment);
                    self.cursor += 1;
                } else {
                    self.dimension(depth + 1)?;
                }
            }
        }
        self.output.push(']');
        Ok(())
    }
}
