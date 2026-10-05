use super::{inference, CassieError, DataType, FieldSchema};

#[derive(Clone, PartialEq)]
pub(super) struct OutputType {
    // The existing descriptor type is retained across set merges. Candidate
    // families/origin do not invent set coercion or a new result type ABI.
    pub(super) data_type: DataType,
    pub(super) candidates: Vec<DataType>,
    pub(super) unresolved_numeric: bool,
}

impl OutputType {
    pub(super) fn fixed(data_type: DataType) -> Self {
        Self {
            candidates: vec![data_type.clone()],
            data_type,
            unresolved_numeric: false,
        }
    }

    pub(super) fn parameter(oid: i32) -> Self {
        match oid {
            700 => Self {
                data_type: DataType::Float,
                candidates: vec![DataType::Float],
                unresolved_numeric: true,
            },
            1700 => Self {
                data_type: DataType::Float,
                candidates: vec![DataType::BigInt, DataType::Float],
                unresolved_numeric: true,
            },
            oid => {
                Self::fixed(inference::data_type_for_parameter_oid(oid).unwrap_or(DataType::Null))
            }
        }
    }

    /// A monotone union: neither AST aliases nor recursive iteration can lose
    /// an existing candidate/origin. The caller decides which operand emits
    /// values (UNION both, EXCEPT/INTERSECT left only).
    pub(super) fn join(&mut self, other: &Self) {
        // Ordinary set coercion remains outside #752. Broaden a domain only
        // when a decoder-only output origin is actually involved.
        if !(self.unresolved_numeric || other.unresolved_numeric) {
            return;
        }
        for candidate in &other.candidates {
            push_distinct(&mut self.candidates, candidate.clone());
        }
        self.unresolved_numeric |= other.unresolved_numeric;
    }

    pub(super) fn common_result(arguments: &[Self]) -> Result<Self, CassieError> {
        let mut candidates = vec![DataType::Null];
        for argument in arguments {
            let mut next = Vec::new();
            for left in &candidates {
                for right in &argument.candidates {
                    let merged = inference::common_case_type(left.clone(), right.clone())
                        .ok_or_else(|| {
                            CassieError::Planner("incompatible result types".to_string())
                        })?;
                    push_distinct(&mut next, merged);
                }
            }
            candidates = next;
        }
        let unresolved = arguments.iter().any(|value| value.unresolved_numeric);
        let independent_peer = arguments.iter().any(|value| {
            !value.unresolved_numeric
                && value
                    .candidates
                    .iter()
                    .any(|kind| !matches!(kind, DataType::Null))
        });
        // COALESCE/CASE inherit an argument's type; a lone adapter plus NULL
        // is not a separately declared result boundary. This conservative
        // case is explicitly flagged for root's focused disposition.
        let fixed = candidates.len() == 1 && independent_peer;
        let data_type = candidates
            .iter()
            .find(|kind| matches!(kind, DataType::Float))
            .or_else(|| candidates.first())
            .cloned()
            .unwrap_or(DataType::Null);
        Ok(Self {
            data_type,
            candidates,
            unresolved_numeric: unresolved && !fixed,
        })
    }
}

pub(super) fn push_distinct(types: &mut Vec<DataType>, data_type: DataType) {
    if !types.contains(&data_type) {
        types.push(data_type);
    }
}

pub(super) fn integer(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::SmallInt | DataType::Int | DataType::BigInt
    )
}

#[derive(Clone, PartialEq)]
pub(super) struct OutputField {
    pub(super) name: String,
    pub(super) value: OutputType,
    pub(super) nullable: bool,
    // Only a wildcard-generated internal identity gets the legacy display
    // name. Explicit aliases to `_id` retain their actual output name.
    pub(super) wildcard_identity: bool,
}

impl OutputField {
    pub(super) fn from_schema(field: FieldSchema) -> Self {
        Self {
            name: field.name,
            value: OutputType::fixed(field.data_type),
            nullable: field.nullable,
            wildcard_identity: false,
        }
    }

    pub(super) fn schema_field(&self) -> FieldSchema {
        let name = if self.wildcard_identity {
            crate::types::row_identity::LEGACY_ID_COLUMN.to_string()
        } else {
            self.name.clone()
        };
        FieldSchema {
            name,
            data_type: self.value.data_type.clone(),
            nullable: self.nullable,
        }
    }
}
