use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vector {
    pub values: Vec<f32>,
}

impl Vector {
    #[must_use]
    pub fn new(values: Vec<f32>) -> Self {
        Self { values }
    }

    #[must_use]
    pub fn dimension(&self) -> usize {
        self.values.len()
    }
}
