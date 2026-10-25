//! Finite JSON inspection functions over the selected native document identities.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JsonFunction {
    Typeof(bool),
    ArrayLength(bool),
    Exists(bool),
    SqlExists,
    SqlInput,
    ObjectKeys(bool),
    BuildObject(bool),
    BuildArray(bool),
    ToJson(bool),
    Set,
    SetLax,
    Insert,
    StripNulls(bool),
    Pretty,
    ExtractPath { binary: bool, as_text: bool },
    PathMatch,
    PathExists,
    PathQueryArray,
    PathQueryFirst,
    PathMatchTz,
    PathExistsTz,
    PathQueryArrayTz,
    PathQueryFirstTz,
}

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    extraction("json_extract_path", false, false),
    extraction("jsonb_extract_path", true, false),
    extraction("json_extract_path_text", false, true),
    extraction("jsonb_extract_path_text", true, true),
    function(
        "json_typeof",
        JsonFunction::Typeof(false),
        FunctionReturnType::Text,
        1,
    ),
    function(
        "jsonb_typeof",
        JsonFunction::Typeof(true),
        FunctionReturnType::Text,
        1,
    ),
    function(
        "json_array_length",
        JsonFunction::ArrayLength(false),
        FunctionReturnType::Int,
        1,
    ),
    function(
        "jsonb_array_length",
        JsonFunction::ArrayLength(true),
        FunctionReturnType::Int,
        1,
    ),
    function(
        "jsonb_exists_any",
        JsonFunction::Exists(false),
        FunctionReturnType::Boolean,
        2,
    ),
    function(
        "jsonb_exists_all",
        JsonFunction::Exists(true),
        FunctionReturnType::Boolean,
        2,
    ),
    function(
        "_cassie_json_exists",
        JsonFunction::SqlExists,
        FunctionReturnType::Boolean,
        3,
    ),
    function(
        "_cassie_json_input",
        JsonFunction::SqlInput,
        FunctionReturnType::Json,
        2,
    ),
    function(
        "json_object_keys",
        JsonFunction::ObjectKeys(false),
        FunctionReturnType::Text,
        1,
    ),
    function(
        "jsonb_object_keys",
        JsonFunction::ObjectKeys(true),
        FunctionReturnType::Text,
        1,
    ),
    constructor(
        "json_build_object",
        JsonFunction::BuildObject(false),
        false,
        false,
    ),
    constructor(
        "json_object",
        JsonFunction::BuildObject(false),
        false,
        false,
    ),
    constructor("jsonb_object", JsonFunction::BuildObject(true), true, false),
    constructor(
        "jsonb_build_object",
        JsonFunction::BuildObject(true),
        true,
        false,
    ),
    constructor(
        "json_build_array",
        JsonFunction::BuildArray(false),
        false,
        false,
    ),
    constructor("json_array", JsonFunction::BuildArray(false), false, false),
    constructor(
        "jsonb_build_array",
        JsonFunction::BuildArray(true),
        true,
        false,
    ),
    constructor("to_json", JsonFunction::ToJson(false), false, true),
    constructor("to_jsonb", JsonFunction::ToJson(true), true, true),
    mutation(
        "jsonb_set",
        JsonFunction::Set,
        FunctionReturnType::Jsonb,
        3,
        4,
    ),
    mutation(
        "jsonb_set_lax",
        JsonFunction::SetLax,
        FunctionReturnType::Jsonb,
        3,
        5,
    ),
    mutation(
        "jsonb_insert",
        JsonFunction::Insert,
        FunctionReturnType::Jsonb,
        3,
        4,
    ),
    mutation(
        "json_strip_nulls",
        JsonFunction::StripNulls(false),
        FunctionReturnType::Json,
        1,
        2,
    ),
    mutation(
        "jsonb_strip_nulls",
        JsonFunction::StripNulls(true),
        FunctionReturnType::Jsonb,
        1,
        2,
    ),
    function(
        "jsonb_pretty",
        JsonFunction::Pretty,
        FunctionReturnType::Text,
        1,
    ),
    function(
        "jsonb_path_match",
        JsonFunction::PathMatch,
        FunctionReturnType::Boolean,
        2,
    ),
    path_function(
        "jsonb_path_exists",
        JsonFunction::PathExists,
        FunctionReturnType::Boolean,
    ),
    path_function(
        "jsonb_path_query_array",
        JsonFunction::PathQueryArray,
        FunctionReturnType::Jsonb,
    ),
    path_function(
        "jsonb_path_query_first",
        JsonFunction::PathQueryFirst,
        FunctionReturnType::Jsonb,
    ),
    path_function(
        "jsonb_path_match_tz",
        JsonFunction::PathMatchTz,
        FunctionReturnType::Boolean,
    ),
    path_function(
        "jsonb_path_exists_tz",
        JsonFunction::PathExistsTz,
        FunctionReturnType::Boolean,
    ),
    path_function(
        "jsonb_path_query_array_tz",
        JsonFunction::PathQueryArrayTz,
        FunctionReturnType::Jsonb,
    ),
    path_function(
        "jsonb_path_query_first_tz",
        JsonFunction::PathQueryFirstTz,
        FunctionReturnType::Jsonb,
    ),
    path_function(
        "jsonb_path_query_tz",
        JsonFunction::PathQueryFirstTz,
        FunctionReturnType::Jsonb,
    ),
    path_function(
        "jsonb_path_query",
        JsonFunction::PathQueryFirst,
        FunctionReturnType::Jsonb,
    ),
];

const fn extraction(name: &'static str, binary: bool, as_text: bool) -> ScalarFunction {
    ScalarFunction {
        id: FunctionId::Json(JsonFunction::ExtractPath { binary, as_text }),
        name,
        arity: FunctionArity::AtLeast(2),
        return_type: if as_text {
            FunctionReturnType::Text
        } else if binary {
            FunctionReturnType::Jsonb
        } else {
            FunctionReturnType::Json
        },
        is_aggregate: false,
        bypass_arity_validation: false,
    }
}

const fn function(
    name: &'static str,
    kind: JsonFunction,
    return_type: FunctionReturnType,
    arity: usize,
) -> ScalarFunction {
    ScalarFunction {
        id: FunctionId::Json(kind),
        name,
        arity: FunctionArity::Exact(arity),
        return_type,
        is_aggregate: false,
        bypass_arity_validation: false,
    }
}

const fn path_function(
    name: &'static str,
    kind: JsonFunction,
    return_type: FunctionReturnType,
) -> ScalarFunction {
    ScalarFunction {
        id: FunctionId::Json(kind),
        name,
        arity: FunctionArity::Range { min: 2, max: 4 },
        return_type,
        is_aggregate: false,
        bypass_arity_validation: false,
    }
}

const fn constructor(
    name: &'static str,
    kind: JsonFunction,
    binary: bool,
    scalar: bool,
) -> ScalarFunction {
    ScalarFunction {
        id: FunctionId::Json(kind),
        name,
        arity: if scalar {
            FunctionArity::Exact(1)
        } else {
            FunctionArity::AtLeast(0)
        },
        return_type: if binary {
            FunctionReturnType::Jsonb
        } else {
            FunctionReturnType::Json
        },
        is_aggregate: false,
        bypass_arity_validation: false,
    }
}

const fn mutation(
    name: &'static str,
    kind: JsonFunction,
    return_type: FunctionReturnType,
    min: usize,
    max: usize,
) -> ScalarFunction {
    ScalarFunction {
        id: FunctionId::Json(kind),
        name,
        arity: FunctionArity::Range { min, max },
        return_type,
        is_aggregate: false,
        bypass_arity_validation: false,
    }
}

pub(super) const fn inputs(kind: JsonFunction) -> Option<&'static [i32]> {
    match kind {
        JsonFunction::Typeof(false)
        | JsonFunction::ArrayLength(false)
        | JsonFunction::ObjectKeys(false) => Some(&[114]),
        JsonFunction::Typeof(true) | JsonFunction::ArrayLength(true) | JsonFunction::Pretty => {
            Some(&[3802])
        }
        JsonFunction::Exists(_) => Some(&[3802, 1009]),
        JsonFunction::SqlExists => Some(&[3802, 4072, 25]),
        JsonFunction::SqlInput => Some(&[25, 16]),
        JsonFunction::ObjectKeys(true) => Some(&[3802]),
        JsonFunction::Set | JsonFunction::Insert => Some(&[3802, 1009, 3802, 16]),
        JsonFunction::SetLax => Some(&[3802, 1009, 3802, 16, 25]),
        JsonFunction::PathMatch => Some(&[3802, 4072]),
        JsonFunction::PathExists
        | JsonFunction::PathQueryArray
        | JsonFunction::PathQueryFirst
        | JsonFunction::PathMatchTz
        | JsonFunction::PathExistsTz
        | JsonFunction::PathQueryArrayTz
        | JsonFunction::PathQueryFirstTz => Some(&[3802, 4072, 3802, 16]),
        JsonFunction::StripNulls(false) => Some(&[114, 16]),
        JsonFunction::StripNulls(true) => Some(&[3802, 16]),
        JsonFunction::BuildObject(_)
        | JsonFunction::BuildArray(_)
        | JsonFunction::ToJson(_)
        | JsonFunction::ExtractPath { .. } => None,
    }
}
