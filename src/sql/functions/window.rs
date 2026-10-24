//! Metadata for window calls stays separate from scalar and aggregate dispatch.

use super::{FunctionArity, FunctionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowArgumentType {
    AnyElement,
    Int4,
    AnyCompatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowInputSignature {
    pub(crate) arity: usize,
    pub(crate) arguments: &'static [WindowArgumentType],
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowFunction {
    pub(crate) id: FunctionId,
    pub(crate) name: &'static str,
    pub(crate) arity: FunctionArity,
    signatures: &'static [WindowInputSignature],
}

impl WindowFunction {
    pub(crate) fn signature(&self, arity: usize) -> Option<&'static [WindowArgumentType]> {
        self.signatures
            .iter()
            .find(|signature| signature.arity == arity)
            .map(|signature| signature.arguments)
    }
}

const LAG_SIGNATURES: &[WindowInputSignature] = &[
    WindowInputSignature {
        arity: 1,
        arguments: &[WindowArgumentType::AnyElement],
    },
    WindowInputSignature {
        arity: 2,
        arguments: &[WindowArgumentType::AnyElement, WindowArgumentType::Int4],
    },
    WindowInputSignature {
        arity: 3,
        arguments: &[
            WindowArgumentType::AnyCompatible,
            WindowArgumentType::Int4,
            WindowArgumentType::AnyCompatible,
        ],
    },
];

const LEAD_SIGNATURES: &[WindowInputSignature] = LAG_SIGNATURES;
const NTH_VALUE_SIGNATURES: &[WindowInputSignature] = &[WindowInputSignature {
    arity: 2,
    arguments: &[WindowArgumentType::AnyElement, WindowArgumentType::Int4],
}];

const FUNCTIONS: &[WindowFunction] = &[
    WindowFunction {
        id: FunctionId::Lag,
        name: "lag",
        arity: FunctionArity::Range { min: 1, max: 3 },
        signatures: LAG_SIGNATURES,
    },
    WindowFunction {
        id: FunctionId::Lead,
        name: "lead",
        arity: FunctionArity::Range { min: 1, max: 3 },
        signatures: LEAD_SIGNATURES,
    },
    WindowFunction {
        id: FunctionId::NthValue,
        name: "nth_value",
        arity: FunctionArity::Exact(2),
        signatures: NTH_VALUE_SIGNATURES,
    },
];

pub(crate) fn window_function(name: &str) -> Option<&'static WindowFunction> {
    let local_name = name.rsplit('.').next()?;
    FUNCTIONS
        .iter()
        .find(|function| function.name.eq_ignore_ascii_case(local_name))
}

#[cfg(test)]
mod tests {
    use super::super::{window_function, FunctionId, WindowArgumentType};

    #[test]
    fn should_define_value_window_input_signatures() {
        // Arrange
        let lag = window_function("lag").expect("LAG must have window metadata");
        let lead = window_function("pg_catalog.LEAD").expect("LEAD must have window metadata");
        let nth_value = window_function("nth_value").expect("NTH_VALUE must have window metadata");

        // Act
        let lag_one = lag.signature(1);
        let lag_two = lag.signature(2);
        let lag_three = lag.signature(3);
        let lead_three = lead.signature(3);
        let nth_two = nth_value.signature(2);

        // Assert
        assert_eq!(lag.id, FunctionId::Lag);
        assert_eq!(lead.id, FunctionId::Lead);
        assert_eq!(nth_value.id, FunctionId::NthValue);
        assert!(lag.arity.matches(1));
        assert!(lag.arity.matches(2));
        assert!(lag.arity.matches(3));
        assert!(!lag.arity.matches(4));
        assert!(nth_value.arity.matches(2));
        assert!(!nth_value.arity.matches(1));
        assert!(crate::sql::functions::function("lag").is_none());
        assert!(crate::sql::functions::function("lead").is_none());
        assert!(crate::sql::functions::function("nth_value").is_none());
        assert_eq!(lag_one, Some(&[WindowArgumentType::AnyElement][..]));
        assert_eq!(
            lag_two,
            Some(&[WindowArgumentType::AnyElement, WindowArgumentType::Int4][..])
        );
        assert_eq!(
            lag_three,
            Some(
                &[
                    WindowArgumentType::AnyCompatible,
                    WindowArgumentType::Int4,
                    WindowArgumentType::AnyCompatible,
                ][..]
            )
        );
        assert_eq!(lead_three, lag_three);
        assert_eq!(
            nth_two,
            Some(&[WindowArgumentType::AnyElement, WindowArgumentType::Int4][..])
        );
        assert!(lag.signature(4).is_none());
        assert!(window_function("not_a_window_function").is_none());
    }

    #[test]
    fn should_reject_nth_value_with_one_argument() {
        // Arrange
        let sql = "SELECT nth_value(1) OVER ()";

        // Act
        let error = crate::sql::parse_statement(sql).expect_err("invalid window arity");

        // Assert
        assert_eq!(
            error.message(),
            "nth_value window function expects two args"
        );
    }

    #[test]
    fn should_reject_lag_with_four_arguments() {
        // Arrange
        let sql = "SELECT lag(1, 2, 3, 4) OVER ()";

        // Act
        let error = crate::sql::parse_statement(sql).expect_err("invalid window arity");

        // Assert
        assert_eq!(
            error.message(),
            "lag window function expects one to three args"
        );
    }
}
