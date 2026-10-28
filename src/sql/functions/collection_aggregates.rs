use super::{CollectionAggregate, FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

pub(crate) const FUNCTIONS: &[ScalarFunction] = &[
    aggregate(
        "array_agg",
        CollectionAggregate::Array,
        FunctionArity::Exact(1),
        FunctionReturnType::Unknown,
    ),
    aggregate(
        "json_agg",
        CollectionAggregate::Json {
            binary: false,
            strict: false,
        },
        FunctionArity::Exact(1),
        FunctionReturnType::Json,
    ),
    aggregate(
        "json_agg_strict",
        CollectionAggregate::Json {
            binary: false,
            strict: true,
        },
        FunctionArity::Exact(1),
        FunctionReturnType::Json,
    ),
    aggregate(
        "jsonb_agg",
        CollectionAggregate::Json {
            binary: true,
            strict: false,
        },
        FunctionArity::Exact(1),
        FunctionReturnType::Jsonb,
    ),
    aggregate(
        "jsonb_agg_strict",
        CollectionAggregate::Json {
            binary: true,
            strict: true,
        },
        FunctionArity::Exact(1),
        FunctionReturnType::Jsonb,
    ),
    aggregate(
        "json_object_agg",
        CollectionAggregate::JsonObject {
            binary: false,
            strict: false,
            unique: false,
        },
        FunctionArity::Exact(2),
        FunctionReturnType::Json,
    ),
    aggregate(
        "json_object_agg_strict",
        CollectionAggregate::JsonObject {
            binary: false,
            strict: true,
            unique: false,
        },
        FunctionArity::Exact(2),
        FunctionReturnType::Json,
    ),
    aggregate(
        "json_object_agg_unique",
        CollectionAggregate::JsonObject {
            binary: false,
            strict: false,
            unique: true,
        },
        FunctionArity::Exact(2),
        FunctionReturnType::Json,
    ),
    aggregate(
        "json_object_agg_unique_strict",
        CollectionAggregate::JsonObject {
            binary: false,
            strict: true,
            unique: true,
        },
        FunctionArity::Exact(2),
        FunctionReturnType::Json,
    ),
    aggregate(
        "jsonb_object_agg",
        CollectionAggregate::JsonObject {
            binary: true,
            strict: false,
            unique: false,
        },
        FunctionArity::Exact(2),
        FunctionReturnType::Jsonb,
    ),
    aggregate(
        "jsonb_object_agg_strict",
        CollectionAggregate::JsonObject {
            binary: true,
            strict: true,
            unique: false,
        },
        FunctionArity::Exact(2),
        FunctionReturnType::Jsonb,
    ),
    aggregate(
        "jsonb_object_agg_unique",
        CollectionAggregate::JsonObject {
            binary: true,
            strict: false,
            unique: true,
        },
        FunctionArity::Exact(2),
        FunctionReturnType::Jsonb,
    ),
    aggregate(
        "jsonb_object_agg_unique_strict",
        CollectionAggregate::JsonObject {
            binary: true,
            strict: true,
            unique: true,
        },
        FunctionArity::Exact(2),
        FunctionReturnType::Jsonb,
    ),
];

const fn aggregate(
    name: &'static str,
    kind: CollectionAggregate,
    arity: FunctionArity,
    return_type: FunctionReturnType,
) -> ScalarFunction {
    ScalarFunction {
        id: FunctionId::CollectionAggregate(kind),
        name,
        arity,
        return_type,
        is_aggregate: true,
        bypass_arity_validation: false,
    }
}
