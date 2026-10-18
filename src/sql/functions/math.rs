//! PostgreSQL 18 mathematical functions (#813) and their result types.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};
use crate::types::extended::ExtendedType;
use crate::types::DataType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathFunction {
    Round,
    Trunc,
    Ceil,
    Ceiling,
    Floor,
    Sign,
    Sqrt,
    Cbrt,
    Exp,
    Ln,
    Log,
    Log10,
    Degrees,
    Radians,
    Sin,
    Cos,
    Tan,
    Cot,
    Asin,
    Acos,
    Atan,
    Atan2,
    Sind,
    Cosd,
    Tand,
    Cotd,
    Asind,
    Acosd,
    Atand,
    Atan2d,
    Sinh,
    Cosh,
    Tanh,
    Asinh,
    Acosh,
    Atanh,
    Pi,
    Erf,
    Erfc,
    Gamma,
    Lgamma,
    Mod,
    Div,
    Scale,
    MinScale,
    TrimScale,
    Factorial,
    WidthBucket,
    Gcd,
    Lcm,
    Random,
    RandomNormal,
    Setseed,
}

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Round),
        name: "round",
        arity: FunctionArity::Range { min: 1, max: 2 },
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Trunc),
        name: "trunc",
        arity: FunctionArity::Range { min: 1, max: 2 },
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Ceil),
        name: "ceil",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Ceiling),
        name: "ceiling",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Floor),
        name: "floor",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Sign),
        name: "sign",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Sqrt),
        name: "sqrt",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Cbrt),
        name: "cbrt",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Exp),
        name: "exp",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Ln),
        name: "ln",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Log),
        name: "log",
        arity: FunctionArity::Range { min: 1, max: 2 },
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Log10),
        name: "log10",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Degrees),
        name: "degrees",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Radians),
        name: "radians",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Sin),
        name: "sin",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Cos),
        name: "cos",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Tan),
        name: "tan",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Cot),
        name: "cot",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Asin),
        name: "asin",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Acos),
        name: "acos",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Atan),
        name: "atan",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Atan2),
        name: "atan2",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Sind),
        name: "sind",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Cosd),
        name: "cosd",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Tand),
        name: "tand",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Cotd),
        name: "cotd",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Asind),
        name: "asind",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Acosd),
        name: "acosd",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Atand),
        name: "atand",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Atan2d),
        name: "atan2d",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Sinh),
        name: "sinh",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Cosh),
        name: "cosh",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Tanh),
        name: "tanh",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Asinh),
        name: "asinh",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Acosh),
        name: "acosh",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Atanh),
        name: "atanh",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Pi),
        name: "pi",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Erf),
        name: "erf",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Erfc),
        name: "erfc",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Gamma),
        name: "gamma",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Lgamma),
        name: "lgamma",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Mod),
        name: "mod",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Div),
        name: "div",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Scale),
        name: "scale",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::MinScale),
        name: "min_scale",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::TrimScale),
        name: "trim_scale",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Factorial),
        name: "factorial",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::WidthBucket),
        name: "width_bucket",
        arity: FunctionArity::Range { min: 2, max: 4 },
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Gcd),
        name: "gcd",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Lcm),
        name: "lcm",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Random),
        name: "random",
        arity: FunctionArity::Range { min: 0, max: 2 },
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::RandomNormal),
        name: "random_normal",
        arity: FunctionArity::Range { min: 0, max: 2 },
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Math(MathFunction::Setseed),
        name: "setseed",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Math,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
];

const NUMERIC: DataType = DataType::Extended(ExtendedType::Numeric(None));

fn is_numeric(data_type: &DataType) -> bool {
    matches!(data_type, DataType::Extended(ExtendedType::Numeric(_)))
}

fn integer_rank(data_type: &DataType) -> Option<u8> {
    match data_type {
        DataType::SmallInt => Some(0),
        DataType::Int => Some(1),
        DataType::BigInt => Some(2),
        _ => None,
    }
}

/// The widest integer type when every argument is an integer (or unknown).
fn common_integer(arguments: &[DataType]) -> Option<DataType> {
    let mut widest: Option<DataType> = None;
    for argument in arguments {
        if matches!(argument, DataType::Null) {
            continue;
        }
        let rank = integer_rank(argument)?;
        if widest
            .as_ref()
            .and_then(integer_rank)
            .is_none_or(|current| rank > current)
        {
            widest = Some(argument.clone());
        }
    }
    Some(widest.unwrap_or(DataType::Int))
}

/// The PostgreSQL result type of `function` over `arguments`.
#[must_use]
pub fn result_type(function: MathFunction, arguments: &[DataType]) -> DataType {
    use MathFunction as F;
    let first = arguments.first().unwrap_or(&DataType::Null);
    match function {
        F::Round | F::Trunc | F::Log if arguments.len() == 2 => NUMERIC,
        F::Round
        | F::Trunc
        | F::Ceil
        | F::Ceiling
        | F::Floor
        | F::Sign
        | F::Sqrt
        | F::Exp
        | F::Ln
        | F::Log
        | F::Log10 => {
            if is_numeric(first) {
                NUMERIC
            } else {
                DataType::Float
            }
        }
        F::Mod | F::Gcd | F::Lcm => common_integer(arguments).unwrap_or(NUMERIC),
        F::Random if arguments.len() == 2 => common_integer(arguments).unwrap_or_else(|| {
            if arguments
                .iter()
                .any(|argument| matches!(argument, DataType::Float))
            {
                DataType::Float
            } else {
                NUMERIC
            }
        }),
        F::Div | F::TrimScale | F::Factorial => NUMERIC,
        F::Scale | F::MinScale | F::WidthBucket => DataType::Int,
        F::Setseed => DataType::Null,
        _ => DataType::Float,
    }
}

/// The parameter types an untyped `$n` argument resolves to, as PostgreSQL's
/// overload resolution selects them; `None` leaves the arguments unresolved.
#[must_use]
pub fn input_oids(function: MathFunction, arity: usize) -> Option<&'static [i32]> {
    use MathFunction as F;
    match (function, arity) {
        (F::Round | F::Trunc, 2) => Some(&[1700, 23]),
        (F::Log | F::Div, 2) => Some(&[1700, 1700]),
        (F::Scale | F::MinScale | F::TrimScale, 1) => Some(&[1700]),
        (F::Factorial, 1) => Some(&[20]),
        (F::Atan2 | F::Atan2d | F::RandomNormal, 2) => Some(&[701, 701]),
        (F::Mod | F::Gcd | F::Lcm | F::Random | F::WidthBucket, _) => None,
        (_, 1) => Some(&[701]),
        _ => None,
    }
}
