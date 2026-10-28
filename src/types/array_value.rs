//! SQL array dimensions, bounds, and row-major element storage.
//!
//! SQL arrays are not ordinary JSON arrays: their dimension lengths and lower
//! bounds affect subscripting and must survive persistence and pgwire.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use super::DataType;
use super::Value;

const STORAGE_KEY: &str = "$cassie_array_v1";
const STORAGE_VERSION: u64 = 1;

/// One PostgreSQL array dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrayDimension {
    /// Number of elements in this dimension.
    pub length: i32,
    /// First valid subscript in this dimension.
    pub lower_bound: i32,
}

/// A SQL array with row-major flat elements and PostgreSQL dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SqlArray {
    dimensions: Vec<ArrayDimension>,
    elements: Vec<Value>,
}

impl SqlArray {
    /// Build a shaped array, validating dimension arithmetic and element count.
    ///
    /// # Errors
    /// Returns an error when dimensions are invalid or do not match the element count.
    pub fn new(dimensions: Vec<ArrayDimension>, elements: Vec<Value>) -> Result<Self, String> {
        let expected = checked_element_count(&dimensions)?;
        if expected != elements.len() {
            return Err(format!(
                "array dimensions require {expected} elements, received {}",
                elements.len()
            ));
        }
        Ok(Self {
            dimensions,
            elements,
        })
    }

    /// Construct a one-dimensional array with PostgreSQL's default lower bound.
    ///
    /// # Errors
    /// Returns an error if the element count cannot fit a SQL array dimension.
    pub fn from_elements(elements: Vec<Value>) -> Result<Self, String> {
        if elements.is_empty() {
            return Ok(Self::empty());
        }
        let length = i32::try_from(elements.len())
            .map_err(|_| "array dimension exceeds INT4".to_string())?;
        Self::new(
            vec![ArrayDimension {
                length,
                lower_bound: 1,
            }],
            elements,
        )
    }

    /// Combine scalar or nested array expressions into one rectangular array.
    ///
    /// # Errors
    /// Returns an error for incompatible shapes, empty nested arrays, or oversized dimensions.
    pub fn from_nested_values(values: Vec<Value>) -> Result<Self, String> {
        if values.is_empty() {
            return Ok(Self::empty());
        }
        let nested_count = values
            .iter()
            .filter(|value| matches!(value, Value::Array(_)))
            .count();
        if nested_count == 0 {
            return Self::from_elements(values);
        }
        if nested_count != values.len() {
            return Err("multidimensional SQL arrays must be rectangular".to_string());
        }
        let length =
            i32::try_from(values.len()).map_err(|_| "array dimension exceeds INT4".to_string())?;
        let mut dimensions = vec![ArrayDimension {
            length,
            lower_bound: 1,
        }];
        let mut elements = Vec::new();
        let mut child_dimensions: Option<Vec<ArrayDimension>> = None;
        for value in values {
            let Value::Array(array) = value else {
                return Err("multidimensional SQL arrays must be rectangular".to_string());
            };
            if array.is_empty() {
                return Err(
                    "multidimensional SQL arrays cannot contain empty subarrays".to_string()
                );
            }
            let current_dimensions = array.dimensions().to_vec();
            if let Some(expected) = child_dimensions.as_ref() {
                if expected != &current_dimensions {
                    return Err("multidimensional SQL arrays must be rectangular".to_string());
                }
            } else {
                dimensions.extend_from_slice(&current_dimensions);
                child_dimensions = Some(current_dimensions);
            }
            elements.extend(array.elements);
        }
        Self::new(dimensions, elements)
    }

    /// Construct an empty PostgreSQL array, which has zero dimensions.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            dimensions: Vec::new(),
            elements: Vec::new(),
        }
    }

    /// Build a 1-based rectangular array from nested JSON array input.
    ///
    /// `element_is_json` disambiguates a JSON-valued array's array documents
    /// from SQL array dimensions: each top-level member remains one element.
    ///
    /// # Errors
    /// Returns an error for non-array input, invalid shapes, or oversized dimensions.
    pub fn from_nested_json(value: &JsonValue, element_is_json: bool) -> Result<Self, String> {
        let JsonValue::Array(values) = value else {
            return Err("SQL array value must be an array".to_string());
        };
        let mut dimensions = Vec::new();
        let mut elements = Vec::new();
        if values.is_empty() {
            return Ok(Self::empty());
        }
        if element_is_json {
            dimensions.push(ArrayDimension {
                length: i32::try_from(values.len())
                    .map_err(|_| "array dimension exceeds INT4".to_string())?,
                lower_bound: 1,
            });
            elements.extend(values.iter().cloned().map(Value::Json));
        } else {
            collect_rectangular(value, 0, &mut dimensions, &mut elements)?;
        }
        if elements.is_empty() {
            return Ok(Self::empty());
        }
        for dimension in &mut dimensions {
            dimension.lower_bound = 1;
        }
        Self::new(dimensions, elements)
    }

    /// Build a typed array from a nested JSON row/document carrier.
    ///
    /// # Errors
    /// Returns an error if the shape or an element is incompatible with the declared type.
    pub fn from_nested_json_typed(
        value: &JsonValue,
        element_type: &DataType,
    ) -> Result<Self, String> {
        let mut element_type = element_type;
        while let DataType::Array(inner) = element_type {
            element_type = inner;
        }
        if let DataType::Vector(vector_dimensions) = element_type {
            let (dimensions, elements) = collect_vector_elements(value, *vector_dimensions)?;
            return if elements.is_empty() {
                Ok(Self::empty())
            } else {
                Self::new(dimensions, elements)
            };
        }
        let element_is_json = matches!(element_type, DataType::Json | DataType::Jsonb);
        let mut array = Self::from_nested_json(value, element_is_json)?;
        if element_is_json {
            return Ok(array);
        }
        for element in array.elements_mut() {
            if matches!(element, Value::Null) {
                continue;
            }
            let json = match element {
                Value::Json(json) => json.clone(),
                other => return Err(format!("unexpected SQL array carrier {other:?}")),
            };
            *element = scalar_from_json(&json, element_type)?;
        }
        Ok(array)
    }

    /// Read the array dimension descriptors.
    #[must_use]
    pub fn dimensions(&self) -> &[ArrayDimension] {
        &self.dimensions
    }

    /// Read the row-major SQL elements.
    #[must_use]
    pub fn elements(&self) -> &[Value] {
        &self.elements
    }

    /// Mutable access to the row-major SQL elements.
    pub fn elements_mut(&mut self) -> &mut [Value] {
        &mut self.elements
    }

    /// Return row-major elements as their JSON/storage payloads.
    #[must_use]
    pub fn elements_as_json(&self) -> Vec<JsonValue> {
        self.elements.iter().map(value_to_json).collect()
    }

    /// Return the number of SQL elements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Whether this is the canonical zero-dimensional empty array.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dimensions.is_empty()
    }

    /// Translate PostgreSQL subscripts into a row-major element offset.
    #[must_use]
    pub fn offset(&self, subscripts: &[i32]) -> Option<usize> {
        if self.dimensions.is_empty() || subscripts.len() != self.dimensions.len() {
            return None;
        }
        let mut offset = 0_usize;
        for (dimension, subscript) in self.dimensions.iter().zip(subscripts) {
            let relative = subscript.checked_sub(dimension.lower_bound)?;
            if relative < 0 || relative >= dimension.length {
                return None;
            }
            offset = offset
                .checked_mul(usize::try_from(dimension.length).ok()?)?
                .checked_add(usize::try_from(relative).ok()?)?;
        }
        Some(offset)
    }

    /// Convert to the nested JSON carrier used by existing row/executor paths.
    #[must_use]
    pub fn to_nested_json(&self) -> JsonValue {
        if self.dimensions.is_empty() {
            return JsonValue::Array(Vec::new());
        }
        // Rebuild recursively from flat elements to preserve all dimensions.
        let mut cursor = 0;
        nested_json(&self.dimensions, &self.elements, &mut cursor)
            .unwrap_or(JsonValue::Array(Vec::new()))
    }

    /// Encode the stable storage envelope consumed by the row codec.
    ///
    /// # Panics
    /// Panics if an array element cannot be serialized to JSON.
    #[must_use]
    pub fn to_storage_json(&self) -> JsonValue {
        let dimensions = self
            .dimensions
            .iter()
            .map(|dimension| {
                serde_json::json!({
                    "length": dimension.length,
                    "lower_bound": dimension.lower_bound,
                })
            })
            .collect::<Vec<_>>();
        let elements = self
            .elements
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .expect("SQL array values are serializable");
        let mut envelope = serde_json::Map::new();
        envelope.insert("version".to_string(), JsonValue::from(STORAGE_VERSION));
        envelope.insert("dimensions".to_string(), JsonValue::Array(dimensions));
        envelope.insert("elements".to_string(), JsonValue::Array(elements));
        let mut object = serde_json::Map::new();
        object.insert(STORAGE_KEY.to_string(), JsonValue::Object(envelope));
        JsonValue::Object(object)
    }

    /// Decode the stable storage envelope, rejecting malformed shapes.
    ///
    /// # Errors
    /// Returns an error for an invalid envelope, element, version, or shape.
    pub fn from_storage_json(value: &JsonValue) -> Result<Self, String> {
        let envelope = value
            .as_object()
            .and_then(|object| object.get(STORAGE_KEY))
            .and_then(JsonValue::as_object)
            .ok_or_else(|| "invalid SQL array storage envelope".to_string())?;
        if envelope.get("version").and_then(JsonValue::as_u64) != Some(STORAGE_VERSION) {
            return Err("unsupported SQL array storage version".to_string());
        }
        let dimension_values = envelope
            .get("dimensions")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| "SQL array dimensions are missing".to_string())?;
        let mut dimensions = Vec::with_capacity(dimension_values.len());
        for value in dimension_values {
            let value = value
                .as_object()
                .ok_or_else(|| "invalid SQL array dimension".to_string())?;
            let length = value
                .get("length")
                .and_then(JsonValue::as_i64)
                .and_then(|length| i32::try_from(length).ok())
                .ok_or_else(|| "invalid SQL array dimension length".to_string())?;
            let lower_bound = value
                .get("lower_bound")
                .and_then(JsonValue::as_i64)
                .and_then(|bound| i32::try_from(bound).ok())
                .ok_or_else(|| "invalid SQL array lower bound".to_string())?;
            dimensions.push(ArrayDimension {
                length,
                lower_bound,
            });
        }
        let values = envelope
            .get("elements")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| "SQL array elements are missing".to_string())?;
        let elements = values
            .iter()
            .cloned()
            .map(serde_json::from_value::<Value>)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "invalid SQL array element in storage envelope".to_string())?;
        Self::new(dimensions, elements)
    }

    /// Convert nested SQL array values to the storage envelope.
    ///
    /// # Errors
    /// Returns an error if the input is not a valid array or storage envelope.
    pub fn storage_json_from_nested(
        value: &JsonValue,
        element_is_json: bool,
    ) -> Result<JsonValue, String> {
        if value
            .as_object()
            .and_then(|object| object.get(STORAGE_KEY))
            .is_some()
        {
            let array = Self::from_storage_json(value)?;
            return Ok(array.to_storage_json());
        }
        Ok(Self::from_nested_json(value, element_is_json)?.to_storage_json())
    }
}

/// Format a shaped SQL array using PostgreSQL text output syntax.
#[must_use]
pub fn format_text(array: &SqlArray, element_type: &DataType) -> Option<String> {
    fn render(
        array: &SqlArray,
        dimension: usize,
        element_type: &DataType,
        offset: &mut usize,
    ) -> Option<String> {
        let shape = array.dimensions().get(dimension)?;
        let mut values = Vec::with_capacity(usize::try_from(shape.length).ok()?);
        for _ in 0..shape.length {
            if dimension + 1 < array.dimensions().len() {
                values.push(render(array, dimension + 1, element_type, offset)?);
            } else {
                let value = array.elements().get(*offset)?;
                *offset += 1;
                if value.is_null() {
                    values.push("NULL".to_string());
                } else {
                    let text = match value {
                        Value::String(text) => text.clone(),
                        Value::Typed(value) => value.text(),
                        Value::Json(value) => value.to_string(),
                        Value::Array(_) => return None,
                        Value::Vector(value) => format!(
                            "[{}]",
                            value
                                .values
                                .iter()
                                .map(std::string::ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(",")
                        ),
                        Value::Bool(value) if matches!(element_type, DataType::Boolean) => {
                            if *value { "t" } else { "f" }.to_string()
                        }
                        Value::Bool(value) => value.to_string(),
                        Value::Int64(value) => value.to_string(),
                        Value::Float64(value) => crate::types::float_text::format(*value),
                        Value::Null => "NULL".to_string(),
                    };
                    values.push(quote_array_element(&text));
                }
            }
        }
        Some(format!("{{{}}}", values.join(",")))
    }
    if array.is_empty() {
        return Some("{}".to_string());
    }
    let mut offset = 0;
    let body = render(array, 0, element_type, &mut offset)?;
    if offset != array.len() {
        return None;
    }
    if array
        .dimensions()
        .iter()
        .all(|dimension| dimension.lower_bound == 1)
    {
        return Some(body);
    }
    let bounds = array
        .dimensions()
        .iter()
        .map(|dimension| {
            Some(format!(
                "[{}:{}]",
                dimension.lower_bound,
                dimension
                    .lower_bound
                    .checked_add(dimension.length)?
                    .checked_sub(1)?
            ))
        })
        .collect::<Option<String>>()?;
    Some(format!("{bounds}={body}"))
}

fn quote_array_element(value: &str) -> String {
    if value.is_empty()
        || value.eq_ignore_ascii_case("NULL")
        || value.chars().any(|character| {
            character.is_whitespace() || matches!(character, '{' | '}' | ',' | '"' | '\\')
        })
    {
        let mut quoted = String::with_capacity(value.len() + 2);
        quoted.push('"');
        for character in value.chars() {
            if matches!(character, '"' | '\\') {
                quoted.push('\\');
            }
            quoted.push(character);
        }
        quoted.push('"');
        quoted
    } else {
        value.to_string()
    }
}

fn checked_element_count(dimensions: &[ArrayDimension]) -> Result<usize, String> {
    if dimensions.is_empty() {
        return Ok(0);
    }
    dimensions.iter().try_fold(1_usize, |count, dimension| {
        if dimension.length <= 0 {
            return Err("nonempty SQL array dimensions must have positive lengths".to_string());
        }
        dimension
            .lower_bound
            .checked_add(dimension.length - 1)
            .ok_or_else(|| "SQL array upper bound exceeds INT4".to_string())?;
        count
            .checked_mul(
                usize::try_from(dimension.length).map_err(|_| {
                    "SQL array dimension length exceeds the platform range".to_string()
                })?,
            )
            .ok_or_else(|| "SQL array element count exceeds the platform range".to_string())
    })
}

fn collect_rectangular(
    value: &JsonValue,
    depth: usize,
    dimensions: &mut Vec<ArrayDimension>,
    elements: &mut Vec<Value>,
) -> Result<(), String> {
    let values = value
        .as_array()
        .ok_or_else(|| "multidimensional SQL arrays must be rectangular".to_string())?;
    let length =
        i32::try_from(values.len()).map_err(|_| "array dimension exceeds INT4".to_string())?;
    if depth == dimensions.len() {
        dimensions.push(ArrayDimension {
            length,
            lower_bound: 1,
        });
    } else if dimensions[depth].length != length {
        return Err("multidimensional SQL arrays must be rectangular".to_string());
    }
    if values.is_empty() {
        return Err("empty nested dimensions are not canonical SQL arrays".to_string());
    }
    if values.iter().all(serde_json::Value::is_array) {
        for value in values {
            collect_rectangular(value, depth + 1, dimensions, elements)?;
        }
    } else if values.iter().any(serde_json::Value::is_array) {
        return Err("multidimensional SQL arrays must be rectangular".to_string());
    } else {
        elements.extend(values.iter().cloned().map(|value| {
            if value.is_null() {
                Value::Null
            } else {
                Value::Json(value)
            }
        }));
    }
    Ok(())
}

fn scalar_from_json(value: &JsonValue, data_type: &DataType) -> Result<Value, String> {
    use DataType as Type;
    match data_type {
        Type::Json | Type::Jsonb => Some(Value::Json(value.clone())),
        Type::Boolean => value.as_bool().map(Value::Bool),
        Type::SmallInt | Type::Int | Type::BigInt | Type::ObjectId(_) => {
            value.as_i64().map(Value::Int64)
        }
        Type::Float => value.as_f64().map(Value::Float64),
        Type::Extended(kind) => {
            if let Ok(typed) = kind.from_json(value) {
                Some(Value::Typed(typed))
            } else {
                value
                    .as_str()
                    .and_then(|text| kind.parse_text(text).ok())
                    .map(Value::Typed)
            }
        }
        Type::Vector(dimensions) => value.as_array().and_then(|values| {
            let parsed = values
                .iter()
                .map(JsonValue::as_f64)
                .map(|number| number.and_then(|value| value.to_string().parse::<f32>().ok()))
                .collect::<Option<Vec<_>>>()?;
            (parsed.len() == *dimensions)
                .then_some(Value::Vector(crate::types::Vector { values: parsed }))
        }),
        Type::Text
        | Type::Name
        | Type::Uuid
        | Type::Date
        | Type::Time
        | Type::Timestamp
        | Type::Bytea
        | Type::Char { .. }
        | Type::Varchar { .. }
        | Type::JsonPath => value.as_str().map(|text| Value::String(text.to_owned())),
        Type::Array(_) | Type::Null => None,
    }
    .ok_or_else(|| {
        format!(
            "array element cannot be represented as {}",
            data_type.type_name()
        )
    })
}

fn nested_json(
    dimensions: &[ArrayDimension],
    elements: &[Value],
    cursor: &mut usize,
) -> Option<JsonValue> {
    let dimension = dimensions.first()?;
    let length = usize::try_from(dimension.length).ok()?;
    let mut values = Vec::with_capacity(length);
    if dimensions.len() == 1 {
        for _ in 0..length {
            values.push(value_to_json(elements.get(*cursor)?));
            *cursor += 1;
        }
    } else {
        for _ in 0..length {
            values.push(nested_json(&dimensions[1..], elements, cursor)?);
        }
    }
    Some(JsonValue::Array(values))
}

fn collect_vector_elements(
    value: &JsonValue,
    vector_dimensions: usize,
) -> Result<(Vec<ArrayDimension>, Vec<Value>), String> {
    if value.is_null() {
        return Ok((Vec::new(), vec![Value::Null]));
    }
    if let Ok(vector) = scalar_from_json(value, &DataType::Vector(vector_dimensions)) {
        return Ok((Vec::new(), vec![vector]));
    }
    let values = value
        .as_array()
        .ok_or_else(|| "array element cannot be represented as VECTOR".to_string())?;
    let length =
        i32::try_from(values.len()).map_err(|_| "array dimension exceeds INT4".to_string())?;
    if values.is_empty() {
        return Ok((
            vec![ArrayDimension {
                length,
                lower_bound: 1,
            }],
            Vec::new(),
        ));
    }
    let mut child_dimensions = None;
    let mut elements = Vec::new();
    for value in values {
        let (dimensions, mut child_elements) = collect_vector_elements(value, vector_dimensions)?;
        if child_dimensions
            .as_ref()
            .is_some_and(|expected| expected != &dimensions)
        {
            return Err("multidimensional SQL arrays must be rectangular".to_string());
        }
        child_dimensions = Some(dimensions);
        elements.append(&mut child_elements);
    }
    let mut dimensions = vec![ArrayDimension {
        length,
        lower_bound: 1,
    }];
    dimensions.extend(child_dimensions.unwrap_or_default());
    Ok((dimensions, elements))
}

fn value_to_json(value: &Value) -> JsonValue {
    match value {
        Value::Null => JsonValue::Null,
        Value::Bool(value) => JsonValue::Bool(*value),
        Value::Int64(value) => JsonValue::from(*value),
        Value::Float64(value) => {
            serde_json::Number::from_f64(*value).map_or(JsonValue::Null, JsonValue::Number)
        }
        Value::String(value) => JsonValue::String(value.clone()),
        Value::Vector(value) => JsonValue::Array(
            value
                .values
                .iter()
                .filter_map(|value| serde_json::Number::from_f64(f64::from(*value)))
                .map(JsonValue::Number)
                .collect(),
        ),
        Value::Json(value) => value.clone(),
        Value::Array(value) => value.to_nested_json(),
        Value::Typed(value) => value.to_json(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ArrayDimension, SqlArray};
    use crate::types::Value;
    use serde_json::Value as JsonValue;

    #[test]
    fn should_preserve_dimensions_and_lower_bounds_in_storage_envelope() {
        // Arrange
        let array = SqlArray::new(
            vec![
                ArrayDimension {
                    length: 2,
                    lower_bound: -1,
                },
                ArrayDimension {
                    length: 2,
                    lower_bound: 4,
                },
            ],
            vec![
                Value::Int64(1),
                Value::Null,
                Value::Int64(3),
                Value::Int64(4),
            ],
        )
        .unwrap();

        // Act
        let restored = SqlArray::from_storage_json(&array.to_storage_json()).unwrap();

        // Assert
        assert_eq!(restored, array);
        assert_eq!(restored.offset(&[-1, 4]), Some(0));
        assert_eq!(restored.offset(&[0, 5]), Some(3));
        assert_eq!(restored.offset(&[-2, 4]), None);
    }

    #[test]
    fn should_distinguish_sql_null_from_json_document_null_in_storage() {
        // Arrange
        let array =
            SqlArray::from_elements(vec![Value::Null, Value::Json(JsonValue::Null)]).unwrap();

        // Act
        let restored = SqlArray::from_storage_json(&array.to_storage_json()).unwrap();

        // Assert
        assert_eq!(
            restored.elements(),
            &[Value::Null, Value::Json(JsonValue::Null)]
        );
    }

    #[test]
    fn should_reject_ragged_nested_json_arrays() {
        // Arrange
        let value = serde_json::json!([[1, 2], [3]]);

        // Act
        let result = SqlArray::from_nested_json(&value, false);

        // Assert
        assert!(result.is_err());
    }
}
