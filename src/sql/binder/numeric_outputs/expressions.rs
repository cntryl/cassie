use super::domain::{integer, push_distinct};
use super::{
    inference, Analyzer, BinaryOp, CassieError, DataType, Expr, FunctionCall, OutputField,
    OutputType,
};
use crate::sql::functions::FunctionReturnType;

impl Analyzer<'_> {
    pub(super) fn expression(
        &self,
        expression: &Expr,
        lookup: &[OutputField],
    ) -> Result<OutputType, CassieError> {
        self.check_controls()?;
        Ok(match expression {
            Expr::Param(index) => {
                OutputType::parameter(self.declared_oids.get(*index).copied().unwrap_or(0))
            }
            Expr::Column(name) => column_type(lookup, name)?,
            // A declared existing-type CAST is the selected output ABI
            // boundary. Ordinary expression/Boolean validation still visits
            // its child in the core path; this does not excuse invalid input.
            Expr::Cast { data_type, .. } => OutputType::fixed(data_type.clone()),
            Expr::Case {
                branches,
                else_expr,
                ..
            } => {
                let mut values = branches
                    .iter()
                    .map(|(_, value)| self.expression(value, lookup))
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(value) = else_expr {
                    values.push(self.expression(value, lookup)?);
                }
                OutputType::common_result(&values)?
            }
            Expr::Function(function) => self.function(function, lookup)?,
            Expr::StringLiteral(_) => OutputType::fixed(DataType::Text),
            Expr::NumberLiteral(_) => OutputType::fixed(DataType::Float),
            Expr::IntegerLiteral(value) => {
                OutputType::fixed(inference::integer_literal_type(*value))
            }
            Expr::BoolLiteral(_)
            | Expr::Exists(_)
            | Expr::IsNull { .. }
            | Expr::InList { .. }
            | Expr::Between { .. }
            | Expr::Not { .. } => OutputType::fixed(DataType::Boolean),
            Expr::Null => OutputType::fixed(DataType::Null),
            Expr::Binary { left, op, right } => match op {
                BinaryOp::And
                | BinaryOp::Or
                | BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Lte
                | BinaryOp::Gt
                | BinaryOp::Gte
                | BinaryOp::Like => OutputType::fixed(DataType::Boolean),
                BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot => {
                    OutputType::fixed(DataType::Float)
                }
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => arithmetic(
                    &self.expression(left, lookup)?,
                    &self.expression(right, lookup)?,
                ),
            },
        })
    }

    pub(super) fn function(
        &self,
        function: &FunctionCall,
        lookup: &[OutputField],
    ) -> Result<OutputType, CassieError> {
        self.check_controls()?;
        let name = function.name.to_ascii_lowercase();
        if let Some(metadata) = self.functions.get(&name) {
            return Ok(OutputType::fixed(metadata.return_type.clone()));
        }
        let Some(metadata) = crate::sql::functions::function(&name) else {
            let arguments = self.function_arguments(function, lookup)?;
            // Matches the existing fallback type but never loses numeric
            // origin through a function whose result policy is unavailable.
            let mut output = OutputType::fixed(DataType::Text);
            output.unresolved_numeric = arguments.iter().any(|arg| arg.unresolved_numeric);
            return Ok(output);
        };
        // COUNT(*) and other fixed-return policies do not export argument
        // values. Ordinary validation checks their arguments separately;
        // this output analyzer must not resolve the `*` pseudo-column.
        if let Some(data_type) = fixed_function_type(metadata.return_type) {
            return Ok(OutputType::fixed(data_type));
        }
        let arguments = self.function_arguments(function, lookup)?;
        Ok(match metadata.return_type {
            FunctionReturnType::FirstNonNullArgument => {
                let mut output = OutputType::common_result(&arguments)?;
                if output.candidates == [DataType::Null] {
                    output.data_type = DataType::Text;
                    output.candidates = vec![DataType::Text];
                }
                output
            }
            FunctionReturnType::NumericArgument | FunctionReturnType::SumArgument => {
                numeric_argument(arguments.first(), metadata.return_type)
            }
            FunctionReturnType::Unknown => {
                let mut output = OutputType::fixed(DataType::Text);
                output.unresolved_numeric = arguments.iter().any(|arg| arg.unresolved_numeric);
                output
            }
            _ => unreachable!("fixed function return policies return before argument analysis"),
        })
    }

    fn function_arguments(
        &self,
        function: &FunctionCall,
        lookup: &[OutputField],
    ) -> Result<Vec<OutputType>, CassieError> {
        function
            .args
            .iter()
            .map(|argument| self.expression(argument, lookup))
            .collect()
    }
}

fn fixed_function_type(policy: FunctionReturnType) -> Option<DataType> {
    match policy {
        FunctionReturnType::Float => Some(DataType::Float),
        FunctionReturnType::Text => Some(DataType::Text),
        FunctionReturnType::Int => Some(DataType::Int),
        FunctionReturnType::BigInt => Some(DataType::BigInt),
        FunctionReturnType::Boolean => Some(DataType::Boolean),
        FunctionReturnType::Timestamp => Some(DataType::Timestamp),
        FunctionReturnType::FirstNonNullArgument
        | FunctionReturnType::NumericArgument
        | FunctionReturnType::SumArgument
        | FunctionReturnType::Unknown => None,
    }
}

fn arithmetic(left: &OutputType, right: &OutputType) -> OutputType {
    let mut candidates = Vec::new();
    for left_type in &left.candidates {
        for right_type in &right.candidates {
            let data_type = if integer(left_type) && integer(right_type) {
                DataType::BigInt
            } else {
                DataType::Float
            };
            push_distinct(&mut candidates, data_type);
        }
    }
    // Unlike COALESCE, arithmetic defines a numeric result operation. A
    // singleton result family is invariant over all admitted carrier kinds.
    let unresolved_numeric =
        (left.unresolved_numeric || right.unresolved_numeric) && candidates.len() != 1;
    let data_type = if candidates.contains(&DataType::Float) {
        DataType::Float
    } else {
        DataType::BigInt
    };
    OutputType {
        data_type,
        candidates,
        unresolved_numeric,
    }
}

fn numeric_argument(argument: Option<&OutputType>, policy: FunctionReturnType) -> OutputType {
    let Some(argument) = argument else {
        return OutputType::fixed(DataType::Float);
    };
    let mut candidates = Vec::new();
    for kind in &argument.candidates {
        let data_type = if policy == FunctionReturnType::SumArgument && integer(kind) {
            DataType::BigInt
        } else if integer(kind) {
            kind.clone()
        } else {
            DataType::Float
        };
        push_distinct(&mut candidates, data_type);
    }
    let data_type = if candidates.contains(&DataType::Float) {
        DataType::Float
    } else {
        candidates.first().cloned().unwrap_or(DataType::Float)
    };
    // A numeric operation supplies a current result family when every
    // admitted carrier kind agrees (700 -> FLOAT). The exact-i64/f64 domain
    // of1700 remains unresolved until a fixed context closes it.
    let unresolved_numeric = argument.unresolved_numeric && candidates.len() != 1;
    OutputType {
        data_type,
        candidates,
        unresolved_numeric,
    }
}

fn column_type(fields: &[OutputField], name: &str) -> Result<OutputType, CassieError> {
    let column = crate::sql::ColumnIdentifierPath::parse(name).map_err(CassieError::Planner)?;
    let found = fields.iter().find(|field| {
        if column.is_qualified() {
            crate::sql::ColumnIdentifierPath::parse(&field.name).is_ok_and(|candidate| {
                candidate.is_qualified() && candidate.lookup_key() == column.lookup_key()
            })
        } else {
            crate::sql::ColumnIdentifierPath::parse(&field.name).is_ok_and(|candidate| {
                !candidate.is_qualified()
                    && candidate.field_lookup_key() == column.field_lookup_key()
            })
        }
    });
    found
        .map(|field| field.value.clone())
        .ok_or_else(|| CassieError::Planner(format!("unresolved output column '{name}'")))
}
