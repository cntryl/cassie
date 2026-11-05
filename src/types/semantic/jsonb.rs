//! Admitted, immutable JSONB comparison keys. No persisted representation changes.
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::mem::size_of;
use std::sync::Arc;

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::extended::TypeInputError;

#[path = "jsonb_number.rs"]
mod number;
use number::Number;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Token {
    Null,
    String(String),
    Number(Number),
    Bool(bool),
    Array(usize),
    Object(usize),
    Field(String),
}

#[derive(Debug)]
struct Owned {
    tokens: Vec<Token>,
    // Backing values must drop before their narrow tracker lease.
    _memory: Option<QueryMemoryReservation>,
}

#[derive(Debug, Clone)]
pub(crate) struct JsonbKey(Arc<Owned>);

impl JsonbKey {
    pub(crate) fn from_text(
        text: &str,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, CassieError> {
        check(controls)?;
        let _parse_memory = controls
            .map(|controls| {
                controls.reserve_query_memory(
                    crate::runtime::accounted::json::serialized_decode_bytes(text.len())?,
                )
            })
            .transpose()?;
        let value = serde_json::from_str(text).map_err(|error| {
            TypeInputError::new(
                "22P02",
                format!("invalid input syntax for type jsonb: {error}"),
            )
        })?;
        Self::from_json(&value, controls)
    }

    pub(crate) fn from_json(
        value: &serde_json::Value,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, CassieError> {
        Self::from_json_observing(value, controls, || {})
    }

    fn from_json_observing(
        value: &serde_json::Value,
        controls: Option<&QueryExecutionControls>,
        mut after_token: impl FnMut(),
    ) -> Result<Self, CassieError> {
        let mut builder = Builder {
            tokens: Vec::new(),
            pending: Vec::new(),
            memory: controls
                .map(|controls| {
                    controls.reserve_query_memory(size_of::<Owned>() + 2 * size_of::<usize>())
                })
                .transpose()?,
            controls,
        };
        builder.push(Work::Value(value))?;
        while let Some(work) = builder.pending.pop() {
            check(controls)?;
            builder.visit(work)?;
            after_token();
            check(controls)?;
        }
        let retained = builder.retained_bytes()?;
        drop(builder.pending);
        if let Some(memory) = builder.memory.as_mut() {
            memory.shrink_to(retained);
        }
        Ok(Self(Arc::new(Owned {
            tokens: builder.tokens,
            _memory: builder.memory,
        })))
    }

    fn root_empty(&self) -> bool {
        matches!(self.0.tokens.first(), Some(Token::Array(0)))
    }
}
impl PartialEq for JsonbKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.tokens == other.0.tokens
    }
}
impl Eq for JsonbKey {}
impl Hash for JsonbKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.tokens.hash(state);
    }
}
impl PartialOrd for JsonbKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for JsonbKey {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .root_empty()
            .cmp(&self.root_empty())
            .then_with(|| self.0.tokens.cmp(&other.0.tokens))
    }
}

enum Work<'a> {
    Value(&'a serde_json::Value),
    Field(&'a str),
}
struct Builder<'a> {
    tokens: Vec<Token>,
    pending: Vec<Work<'a>>,
    memory: Option<QueryMemoryReservation>,
    controls: Option<&'a QueryExecutionControls>,
}
impl<'a> Builder<'a> {
    fn admit(&mut self, bytes: usize) -> Result<(), CassieError> {
        if let Some(memory) = self.memory.as_mut() {
            memory.try_grow(bytes)?;
        }
        Ok(())
    }
    fn push(&mut self, work: Work<'a>) -> Result<(), CassieError> {
        if self.pending.len() == self.pending.capacity() {
            let extra = self.pending.capacity().max(4);
            self.admit(mul(extra, size_of::<Work<'_>>())?)?;
            self.pending.try_reserve_exact(extra).map_err(allocation)?;
        }
        self.pending.push(work);
        Ok(())
    }
    fn token(&mut self, token: Token) -> Result<(), CassieError> {
        if self.tokens.len() == self.tokens.capacity() {
            let extra = self.tokens.capacity().max(4);
            self.admit(mul(extra, size_of::<Token>())?)?;
            self.tokens.try_reserve_exact(extra).map_err(allocation)?;
        }
        self.tokens.push(token);
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<String, CassieError> {
        if text.contains('\0') {
            return Err(TypeInputError::new(
                "22P05",
                "unsupported Unicode escape sequence: \\u0000".into(),
            )
            .into());
        }
        self.admit(text.len())?;
        let mut output = String::new();
        output.try_reserve_exact(text.len()).map_err(allocation)?;
        output.push_str(text);
        Ok(output)
    }
    fn visit(&mut self, work: Work<'a>) -> Result<(), CassieError> {
        use serde_json::Value;
        match work {
            Work::Field(name) => {
                let name = self.text(name)?;
                self.token(Token::Field(name))
            }
            Work::Value(value) => match value {
                Value::Null => self.token(Token::Null),
                Value::Bool(value) => self.token(Token::Bool(*value)),
                Value::String(text) => {
                    let text = self.text(text)?;
                    self.token(Token::String(text))
                }
                Value::Number(value) => {
                    // Canonical coefficient, native BigInt, and formatter workspace coexist.
                    self.admit(
                        mul(value.as_str().len().max(1), 8)?
                            .checked_add(256)
                            .ok_or_else(overflow)?,
                    )?;
                    let number = Number::from_validated(value.as_str(), self.controls)?;
                    check(self.controls)?;
                    self.token(Token::Number(number))
                }
                Value::Array(values) => {
                    self.token(Token::Array(values.len()))?;
                    for value in values.iter().rev() {
                        check(self.controls)?;
                        self.push(Work::Value(value))?;
                    }
                    Ok(())
                }
                Value::Object(fields) => {
                    self.token(Token::Object(fields.len()))?;
                    let bytes = mul(fields.len(), size_of::<(&str, &Value)>())?;
                    self.admit(bytes)?;
                    let mut ordered = Vec::new();
                    ordered
                        .try_reserve_exact(fields.len())
                        .map_err(allocation)?;
                    for (key, value) in fields {
                        check(self.controls)?;
                        ordered.push((key.as_str(), value));
                    }
                    let mut interrupted = None;
                    ordered.sort_unstable_by(|(left, _), (right, _)| {
                        if interrupted.is_none() {
                            interrupted = check(self.controls).err();
                        }
                        left.len()
                            .cmp(&right.len())
                            .then_with(|| left.as_bytes().cmp(right.as_bytes()))
                    });
                    if let Some(error) = interrupted {
                        return Err(error);
                    }
                    check(self.controls)?;
                    for (key, value) in ordered.into_iter().rev() {
                        self.push(Work::Value(value))?;
                        self.push(Work::Field(key))?;
                    }
                    Ok(())
                }
            },
        }
    }
    fn retained_bytes(&self) -> Result<usize, CassieError> {
        self.tokens.iter().try_fold(
            mul(self.tokens.capacity(), size_of::<Token>())?
                .checked_add(size_of::<Owned>() + 2 * size_of::<usize>())
                .ok_or_else(overflow)?,
            |bytes, token| {
                bytes
                    .checked_add(match token {
                        Token::String(text) | Token::Field(text) => text.capacity(),
                        Token::Number(number) => number.heap_bytes(),
                        _ => 0,
                    })
                    .ok_or_else(overflow)
            },
        )
    }
}
fn check(controls: Option<&QueryExecutionControls>) -> Result<(), CassieError> {
    controls.map_or(Ok(()), QueryExecutionControls::check_interruption)
}
fn mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(overflow)
}
fn overflow() -> CassieError {
    CassieError::ResourceLimit("JSONB semantic key accounting overflow".into())
}
fn allocation(error: std::collections::TryReserveError) -> CassieError {
    CassieError::ResourceLimit(format!("JSONB semantic key allocation: {error}"))
}

#[cfg(test)]
#[path = "jsonb_tests.rs"]
mod tests;
