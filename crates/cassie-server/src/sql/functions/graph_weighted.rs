//! Identity and output shape of `graph_weighted_paths` (#1258).

use crate::types::extended::ExtendedType;
use crate::types::DataType;

/// The function's name; it is unqualified in the contract.
pub(crate) const NAME: &str = "graph_weighted_paths";

/// Argument names in positional order. The first five are required.
pub(crate) const ARGUMENTS: [&str; 9] = [
    "graph",
    "source",
    "target",
    "weight_property",
    "max_hops",
    "direction",
    "edge_labels",
    "max_cost",
    "k",
];

/// How many leading arguments have no default.
pub(crate) const REQUIRED: usize = 5;

/// Whether `name` is the weighted path function.
pub(crate) fn is_weighted_paths(name: &str) -> bool {
    name.eq_ignore_ascii_case(NAME)
}

fn numeric() -> DataType {
    DataType::Extended(ExtendedType::Numeric(None))
}

/// Output columns; `path` is typed by the bound database's `graph_path`
/// descriptor when it is known and untyped before.
pub(crate) fn columns(path: Option<DataType>) -> Vec<(String, DataType)> {
    vec![
        ("path_rank".to_owned(), DataType::BigInt),
        ("cost".to_owned(), numeric()),
        ("path".to_owned(), path.unwrap_or(DataType::Null)),
        (
            "edge_costs".to_owned(),
            DataType::Array(Box::new(numeric())),
        ),
    ]
}
