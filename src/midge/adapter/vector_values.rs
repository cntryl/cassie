use crate::types::{Value, Vector};

impl From<&Value> for Vector {
    fn from(value: &Value) -> Self {
        match value {
            Value::Vector(value) => value.clone(),
            _ => Vector::new(Vec::new()),
        }
    }
}

#[must_use]
pub fn vector_from_json(value: &serde_json::Value) -> Option<Vector> {
    let array = value.as_array()?;
    let mut numbers = Vec::with_capacity(array.len());
    for number in array {
        numbers.push(number.as_f64()?.to_string().parse::<f32>().ok()?);
    }
    Some(Vector::new(numbers))
}
