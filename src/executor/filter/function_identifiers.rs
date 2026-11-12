//! Identifier quoting and legacy `format_type` display policy.

pub(in crate::executor::filter) fn quote_identifier(value: &str) -> String {
    if !identifier_is_unquoted(value) {
        return format!("\"{}\"", value.replace('"', "\"\""));
    }
    value.to_string()
}

pub(in crate::executor::filter) fn identifier_is_unquoted(value: &str) -> bool {
    let simple = value
        .chars()
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_lowercase())
        && value
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_lowercase() || ch.is_ascii_digit());
    simple && !is_reserved_identifier(value)
}

fn is_reserved_identifier(value: &str) -> bool {
    const POSTGRES_RESERVED_IDENTIFIERS: &[&str] = &[
        "all",
        "analyse",
        "analyze",
        "and",
        "any",
        "array",
        "as",
        "asc",
        "asymmetric",
        "both",
        "case",
        "cast",
        "check",
        "collate",
        "column",
        "constraint",
        "create",
        "current_catalog",
        "current_date",
        "current_role",
        "current_time",
        "current_timestamp",
        "current_user",
        "default",
        "deferrable",
        "desc",
        "distinct",
        "do",
        "else",
        "end",
        "except",
        "false",
        "fetch",
        "for",
        "foreign",
        "from",
        "grant",
        "group",
        "having",
        "in",
        "initially",
        "intersect",
        "into",
        "lateral",
        "leading",
        "limit",
        "localtime",
        "localtimestamp",
        "not",
        "null",
        "offset",
        "on",
        "only",
        "or",
        "order",
        "placing",
        "primary",
        "references",
        "returning",
        "select",
        "session_user",
        "some",
        "symmetric",
        "system_user",
        "table",
        "then",
        "to",
        "trailing",
        "true",
        "union",
        "unique",
        "user",
        "using",
        "variadic",
        "when",
        "where",
        "window",
        "with",
    ];
    POSTGRES_RESERVED_IDENTIFIERS.binary_search(&value).is_ok()
}

pub(super) fn format_type_oid(oid: i64, typmod: i64) -> String {
    match oid {
        16 => "boolean".to_string(),
        17 => "bytea".to_string(),
        20 => "bigint".to_string(),
        21 => "smallint".to_string(),
        23 => "integer".to_string(),
        25 => "text".to_string(),
        114 => "json".to_string(),
        701 => "double precision".to_string(),
        1042 => format_character_type("character", typmod),
        1043 => format_character_type("character varying", typmod),
        1082 => "date".to_string(),
        1083 => "time without time zone".to_string(),
        1114 => "timestamp without time zone".to_string(),
        2950 => "uuid".to_string(),
        oid if (34_000..50_000).contains(&oid) => "array".to_string(),
        _ => oid.to_string(),
    }
}

fn format_character_type(name: &str, typmod: i64) -> String {
    if typmod >= 4 {
        format!("{name}({})", typmod - 4)
    } else {
        name.to_string()
    }
}
