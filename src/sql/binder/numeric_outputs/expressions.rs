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
            Expr::Cast {
                data_type,
                identity,
                ..
            } => {
                if *identity == crate::sql::ast::CastIdentity::Numeric {
                    let mut output = OutputType::fixed(data_type.clone());
                    output.proven_float8 = false;
                    output
                } else if let Some(original) =
                    super::super::result_coercion::original_operand(expression)
                {
                    self.expression(original, lookup)?
                } else {
                    OutputType::fixed(data_type.clone())
                }
            }
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
            Expr::NumericLiteral(_) => OutputType::fixed(DataType::Extended(
                crate::types::extended::ExtendedType::Numeric(None),
            )),
            Expr::StringLiteral(_) => OutputType::fixed(DataType::Text),
            Expr::NumberLiteral(_) => {
                let mut output = OutputType::fixed(DataType::Float);
                output.proven_float8 = false;
                output
            }
            Expr::UnaryNumeric { expr, op } => {
                if matches!(
                    op,
                    crate::sql::ast::UnaryNumericOp::SquareRoot
                        | crate::sql::ast::UnaryNumericOp::CubeRoot
                ) || matches!(
                    op,
                    crate::sql::ast::UnaryNumericOp::Plus
                        | crate::sql::ast::UnaryNumericOp::Absolute
                ) && super::super::unary_numeric::is_unknown_operand(expr)
                {
                    OutputType::fixed(DataType::Float)
                } else {
                    self.expression(expr, lookup)?
                }
            }
            Expr::IntegerNegate { width, .. } => OutputType::fixed(width.data_type()),
            Expr::IntegerLiteral(value) => {
                OutputType::fixed(inference::integer_literal_type(*value))
            }
            Expr::RowCompare(_)
            | Expr::BoolLiteral(_)
            | Expr::Exists(_)
            | Expr::IsNull { .. }
            | Expr::TruthTest { .. }
            | Expr::InList { .. }
            | Expr::Between { .. }
            | Expr::Like { .. }
            | Expr::Not { .. } => OutputType::fixed(DataType::Boolean),
            Expr::Null => OutputType::fixed(DataType::Null),
            Expr::Subquery(subquery) => {
                OutputType::fixed(subquery.result_type().unwrap_or(DataType::Null))
            }
            Expr::Binary { left, op, right } => self.binary_output(left, op, right, lookup)?,
        })
    }

    fn binary_output(
        &self,
        left: &Expr,
        op: &BinaryOp,
        right: &Expr,
        lookup: &[OutputField],
    ) -> Result<OutputType, CassieError> {
        if let Some(operator) = op.typed_operator() {
            let left_type = self.expression(left, lookup)?.data_type;
            let right_type = self.expression(right, lookup)?.data_type;
            if let Some(kind) =
                crate::types::extended::operations::operator_type(operator, &left_type, &right_type)
            {
                return Ok(OutputType::fixed(kind));
            }
        }
        Ok(match op {
            BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::IsDistinctFrom
            | BinaryOp::IsNotDistinctFrom
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte
            | BinaryOp::Typed { .. }
            | BinaryOp::Quantified { .. }
            | BinaryOp::Like => OutputType::fixed(DataType::Boolean),
            BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot => {
                OutputType::fixed(DataType::Float)
            }
            BinaryOp::Power => {
                let left = self.expression(left, lookup)?;
                let right = self.expression(right, lookup)?;
                crate::types::extended::numeric_types::arithmetic(
                    &left.data_type,
                    &right.data_type,
                    true,
                )
                .map_or_else(|| OutputType::fixed(DataType::Float), OutputType::fixed)
            }
            BinaryOp::Concat => OutputType::fixed(DataType::Text),
            BinaryOp::ByteaConcat => OutputType::fixed(DataType::Bytea),
            BinaryOp::Bitwise { operation } if operation.is_shift() => {
                self.expression(left, lookup)?
            }
            BinaryOp::Integer { operation, width } => OutputType::fixed(if operation.is_shift() {
                width.data_type()
            } else {
                DataType::BigInt
            }),
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Modulo
            | BinaryOp::Bitwise { .. } => {
                let left = self.expression(left, lookup)?;
                let right = self.expression(right, lookup)?;
                if let Some(data_type) = crate::sql::functions::temporal_binary_type(
                    op,
                    Some(&left.data_type),
                    Some(&right.data_type),
                ) {
                    return Ok(OutputType::fixed(data_type));
                }
                crate::types::extended::numeric_types::arithmetic(
                    &left.data_type,
                    &right.data_type,
                    matches!(op, BinaryOp::Modulo),
                )
                .map_or_else(|| arithmetic(&left, &right), OutputType::fixed)
            }
        })
    }

    pub(super) fn function(
        &self,
        function: &FunctionCall,
        lookup: &[OutputField],
    ) -> Result<OutputType, CassieError> {
        self.check_controls()?;
        let name = function.name.to_ascii_lowercase();
        if let Some(metadata) = self.functions.get(&name).filter(|metadata| {
            !crate::sql::functions::has_distinct_builtin_arity(
                &name,
                function.args.len(),
                metadata.args.len(),
            )
        }) {
            return Ok(OutputType::fixed(metadata.return_type.clone()));
        }
        let Some(metadata) = crate::sql::functions::function(&name) else {
            let arguments = self.function_arguments(function, lookup)?;
            let argument_types: Vec<DataType> = arguments
                .iter()
                .map(|argument| argument.data_type.clone())
                .collect();
            if let Some(kind) = crate::sql::functions::hashing::return_type(&name).or_else(|| {
                crate::types::extended::operations::function_type(&name, &argument_types)
            }) {
                return Ok(OutputType::fixed(kind));
            }
            // Matches the existing fallback type but never loses numeric
            // origin through a function whose result policy is unavailable.
            let mut output = OutputType::fixed(DataType::Text);
            output.unresolved_numeric = arguments.iter().any(|arg| arg.unresolved_numeric);
            return Ok(output);
        };
        // COUNT(*) and other fixed-return policies do not export argument
        // values. Ordinary validation checks their arguments separately;
        // this output analyzer must not resolve the `*` pseudo-column.
        if let Some(output) = self.ordered_set_function(function, metadata.id, lookup)? {
            return Ok(output);
        }
        if let Some(output) = self.temporal_function(function, metadata.id, lookup)? {
            return Ok(output);
        }
        if let Some(output) = self.numeric_function(function, metadata.id, lookup)? {
            return Ok(output);
        }
        if let Some(data_type) = fixed_function_type(metadata.return_type) {
            let mut output = OutputType::fixed(data_type);
            if output.data_type == DataType::Float {
                // Carrier metadata alone does not resolve overloaded NUMERIC
                // signatures. AVG(float8) is a selected declared input.
                output.proven_float8 = name == "avg"
                    && function
                        .args
                        .first()
                        .map(|argument| self.expression(argument, lookup))
                        .transpose()?
                        .is_some_and(|argument| argument.proven_float8);
            }
            return Ok(output);
        }
        let arguments = self.function_arguments(function, lookup)?;
        Ok(match metadata.return_type {
            FunctionReturnType::FirstComparedArgument => {
                let common = OutputType::common_result(&arguments)?;
                if common.candidates.contains(&DataType::Float) {
                    common
                } else {
                    arguments
                        .first()
                        .filter(|first| first.data_type != DataType::Null)
                        .cloned()
                        .unwrap_or_else(|| {
                            if common.data_type == DataType::Null {
                                OutputType::fixed(DataType::Text)
                            } else {
                                common
                            }
                        })
                }
            }
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
            FunctionReturnType::Math => {
                let crate::sql::functions::FunctionId::Math(math) = metadata.id else {
                    unreachable!("math return policies belong to math functions")
                };
                let types = arguments
                    .iter()
                    .map(|argument| argument.data_type.clone())
                    .collect::<Vec<_>>();
                let mut output =
                    OutputType::fixed(crate::sql::functions::math::result_type(math, &types));
                output.unresolved_numeric = arguments.iter().any(|arg| arg.unresolved_numeric)
                    && output.data_type != DataType::Float;
                output
            }
            FunctionReturnType::Unknown => {
                let mut output = OutputType::fixed(DataType::Text);
                output.unresolved_numeric = arguments.iter().any(|arg| arg.unresolved_numeric);
                output
            }
            _ => unreachable!("fixed function return policies return before argument analysis"),
        })
    }

    fn ordered_set_function(
        &self,
        function: &FunctionCall,
        id: crate::sql::functions::FunctionId,
        lookup: &[OutputField],
    ) -> Result<Option<OutputType>, CassieError> {
        use crate::sql::functions::FunctionId;

        if !matches!(
            id,
            FunctionId::Mode | FunctionId::PercentileCont | FunctionId::PercentileDisc
        ) {
            return Ok(None);
        }
        let Some(order) = function.order_by.first() else {
            return Ok(None);
        };
        let ordered_type = self.expression(&order.expr, lookup)?.data_type;
        if id == FunctionId::Mode {
            return Ok(Some(OutputType::fixed(ordered_type)));
        }
        let element_type = match id {
            FunctionId::PercentileCont
                if matches!(
                    ordered_type,
                    DataType::Extended(crate::types::extended::ExtendedType::Interval { .. })
                ) =>
            {
                ordered_type
            }
            FunctionId::PercentileCont => DataType::Float,
            FunctionId::PercentileDisc => ordered_type,
            _ => return Ok(None),
        };
        let is_array = function.args.first().is_some_and(|fraction| {
            matches!(
                fraction,
                Expr::Subquery(subquery)
                    if matches!(subquery.kind, crate::sql::ast::SubqueryKind::Array)
            ) || self
                .expression(fraction, lookup)
                .is_ok_and(|output| matches!(output.data_type, DataType::Array(_)))
        });
        Ok(Some(OutputType::fixed(if is_array {
            DataType::Array(Box::new(element_type))
        } else {
            element_type
        })))
    }

    /// Date/time builtins, and `sum`/`avg` over `interval` (an `interval`).
    fn temporal_function(
        &self,
        function: &FunctionCall,
        id: crate::sql::functions::FunctionId,
        lookup: &[OutputField],
    ) -> Result<Option<OutputType>, CassieError> {
        use crate::sql::functions::FunctionId;
        if !matches!(id, FunctionId::Sum | FunctionId::Avg | FunctionId::Temporal) {
            return Ok(None);
        }
        let arguments = self.function_arguments(function, lookup)?;
        if id == FunctionId::Temporal {
            let argument_type = |index: usize| {
                arguments
                    .get(index)
                    .map(|argument| argument.data_type.clone())
            };
            return Ok(crate::sql::functions::temporal_return_type(
                &function.name.to_ascii_lowercase(),
                &function.args,
                &argument_type,
            )
            .map(OutputType::fixed));
        }
        Ok(arguments
            .first()
            .filter(|argument| {
                matches!(
                    argument.data_type,
                    DataType::Extended(crate::types::extended::ExtendedType::Interval { .. })
                )
            })
            .map(|argument| OutputType::fixed(argument.data_type.clone())))
    }

    /// `power` and `avg` result types that depend on a `numeric` argument.
    fn numeric_function(
        &self,
        function: &FunctionCall,
        id: crate::sql::functions::FunctionId,
        lookup: &[OutputField],
    ) -> Result<Option<OutputType>, CassieError> {
        if id == crate::sql::functions::FunctionId::Power && function.args.len() == 2 {
            let left = self.expression(&function.args[0], lookup)?;
            let right = self.expression(&function.args[1], lookup)?;
            if let Some(data_type) = crate::types::extended::numeric_types::arithmetic(
                &left.data_type,
                &right.data_type,
                true,
            ) {
                return Ok(Some(OutputType::fixed(data_type)));
            }
        }
        if id == crate::sql::functions::FunctionId::Avg {
            if let Some(argument) = function
                .args
                .first()
                .map(|argument| self.expression(argument, lookup))
                .transpose()?
                .filter(|argument| !argument.unresolved_numeric)
            {
                let data_type = crate::types::extended::numeric_types::avg(&argument.data_type);
                let mut output = OutputType::fixed(data_type);
                output.proven_float8 = argument.proven_float8;
                return Ok(Some(output));
            }
        }
        if crate::sql::functions::statistical::is_statistical_function(id) {
            let argument = function
                .args
                .first()
                .map(|argument| self.expression(argument, lookup))
                .transpose()?;
            return Ok(Some(statistical_argument(argument.as_ref())));
        }
        Ok(None)
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
        FunctionReturnType::Bytea => Some(DataType::Bytea),
        FunctionReturnType::Int => Some(DataType::Int),
        FunctionReturnType::BigInt => Some(DataType::BigInt),
        FunctionReturnType::Boolean => Some(DataType::Boolean),
        FunctionReturnType::Timestamp => Some(DataType::Timestamp),
        FunctionReturnType::ObjectId(kind) => Some(DataType::ObjectId(kind)),
        FunctionReturnType::FirstNonNullArgument
        | FunctionReturnType::FirstComparedArgument
        | FunctionReturnType::NumericArgument
        | FunctionReturnType::SumArgument
        | FunctionReturnType::Math
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
        proven_float8: left.proven_float8 || right.proven_float8,
    }
}

fn numeric_argument(argument: Option<&OutputType>, policy: FunctionReturnType) -> OutputType {
    let Some(argument) = argument else {
        let mut output = OutputType::fixed(DataType::Float);
        output.proven_float8 = false;
        return output;
    };
    let mut candidates = Vec::new();
    for kind in &argument.candidates {
        let data_type = if policy == FunctionReturnType::SumArgument {
            crate::types::extended::numeric_types::sum(kind)
        } else if matches!(kind, DataType::Extended(_)) || integer(kind) {
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
        proven_float8: argument.proven_float8,
    }
}

fn statistical_argument(argument: Option<&OutputType>) -> OutputType {
    let Some(argument) = argument else {
        return OutputType::fixed(DataType::Float);
    };
    let mut candidates = Vec::new();
    for candidate in &argument.candidates {
        push_distinct(
            &mut candidates,
            crate::types::extended::numeric_types::statistical(candidate),
        );
    }
    let data_type = if candidates.contains(&DataType::Float) {
        DataType::Float
    } else {
        candidates.first().cloned().unwrap_or(DataType::Float)
    };
    OutputType {
        data_type,
        unresolved_numeric: argument.unresolved_numeric && candidates.len() != 1,
        proven_float8: candidates == [DataType::Float],
        candidates,
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
