//! PostgreSQL's FLOAT8 Youngs–Cramer transition for a proven cast carrier.

use crate::app::CassieError;
use crate::types::extended::TypeInputError;

use super::QueryError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FloatCast {
    Float8,
    Real,
}

#[derive(Clone, Copy, Default)]
pub(super) struct PgFloatMoments {
    count: f64,
    sum: f64,
    sxx: f64,
}

impl PgFloatMoments {
    pub(super) fn update(&mut self, value: f64) -> Result<(), QueryError> {
        let n = self.count + 1.0;
        let sum = self.sum + value;
        let mut sxx = self.sxx;
        if self.count > 0.0 {
            let previous_n = self.count;
            let tmp = value * n - sum;
            sxx += tmp * tmp / (n * previous_n);
            if sum.is_infinite() || sxx.is_infinite() {
                if !self.sum.is_infinite() && !value.is_infinite() {
                    return Err(overflow());
                }
                sxx = f64::NAN;
            }
        } else if !value.is_finite() {
            sxx = f64::NAN;
        }
        self.count = n;
        self.sum = sum;
        self.sxx = sxx;
        Ok(())
    }

    pub(super) fn merge(&mut self, other: &Self) -> Result<(), QueryError> {
        if other.count == 0.0 {
            return Ok(());
        }
        if self.count == 0.0 {
            *self = *other;
            return Ok(());
        }
        let left_n = self.count;
        let right_n = other.count;
        let n = left_n + right_n;
        let sum = self.sum + other.sum;
        if sum.is_infinite() && !self.sum.is_infinite() && !other.sum.is_infinite() {
            return Err(overflow());
        }
        let tmp = self.sum / left_n - other.sum / right_n;
        let sxx = self.sxx + other.sxx + left_n * right_n * tmp * tmp / n;
        if sxx.is_infinite() && !self.sxx.is_infinite() && !other.sxx.is_infinite() {
            return Err(overflow());
        }
        self.count = n;
        self.sum = sum;
        self.sxx = sxx;
        Ok(())
    }

    pub(super) fn result(self, sample: bool, standard_deviation: bool) -> Option<f64> {
        if self.count == 0.0 || sample && self.count <= 1.0 {
            return None;
        }
        let denominator = if sample { self.count - 1.0 } else { self.count };
        let variance = self.sxx / denominator;
        Some(if standard_deviation {
            variance.sqrt()
        } else {
            variance
        })
    }
}

fn overflow() -> QueryError {
    QueryError::Cassie(CassieError::TypeInput(TypeInputError {
        sqlstate: "22003",
        message: "value out of range: overflow".to_owned(),
    }))
}
