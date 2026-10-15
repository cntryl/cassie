//! PostgreSQL keyword forms mapped to the existing scalar-call AST.

use super::super::clauses::find_top_level_keyword_span;
use super::super::expr::parse_expression;
use super::super::{lexical, Expr, SqlError};

pub(super) fn arguments(name: &str, body: &str) -> Result<Option<(String, Vec<Expr>)>, SqlError> {
    if name.eq_ignore_ascii_case("position") {
        let Some((needle, text)) = split(body, "in") else {
            return Err(SqlError::syntax(
                "POSITION requires substring IN string".into(),
            ));
        };
        return Ok(Some((
            name.to_owned(),
            vec![parse_expression(text)?, parse_expression(needle)?],
        )));
    }
    if name.eq_ignore_ascii_case("substring") {
        return substring(body)
            .map(|arguments| arguments.map(|arguments| (name.to_owned(), arguments)));
    }
    if name.eq_ignore_ascii_case("trim") {
        let (direction, body) = ["both", "leading", "trailing"]
            .into_iter()
            .find_map(|keyword| {
                lexical::pattern_end(body, 0, keyword)
                    .map(|end| (keyword, lexical::trim_separators(&body[end..])))
            })
            .unwrap_or(("both", body));
        if let Some((characters, text)) = split(body, "from") {
            let mut args = vec![parse_expression(text)?];
            if !lexical::trim_separators(characters).is_empty() {
                args.push(parse_expression(characters)?);
            }
            let name = match direction {
                "leading" => "ltrim",
                "trailing" => "rtrim",
                _ => "btrim",
            };
            return Ok(Some((name.to_owned(), args)));
        }
        if body.is_empty() {
            return Err(SqlError::syntax("TRIM requires a string expression".into()));
        }
        // Comma-separated scalar syntax is handled by the ordinary call parser.
    }
    Ok(None)
}

fn substring(body: &str) -> Result<Option<Vec<Expr>>, SqlError> {
    let (text, first, rest) =
        if let Some((position, end)) = find_top_level_keyword_span(body, 0, "from") {
            // FOR may precede FROM in the standard grammar.
            if let Some((for_position, for_end)) = find_top_level_keyword_span(body, 0, "for") {
                if for_position < position {
                    return Ok(Some(vec![
                        parse_expression(&body[..for_position])?,
                        parse_expression(&body[end..])?,
                        parse_expression(&body[for_end..position])?,
                    ]));
                }
            }
            (&body[..position], "from", &body[end..])
        } else if let Some((text, length)) = split(body, "for") {
            (text, "for", length)
        } else {
            return Ok(None);
        };
    let mut args = vec![parse_expression(text)?];
    if first == "for" {
        args.push(Expr::IntegerLiteral(1));
        args.push(parse_expression(rest)?);
    } else if let Some((start, length)) = split(rest, "for") {
        args.push(parse_expression(start)?);
        args.push(parse_expression(length)?);
    } else {
        args.push(parse_expression(rest)?);
    }
    Ok(Some(args))
}

fn split<'a>(body: &'a str, keyword: &str) -> Option<(&'a str, &'a str)> {
    find_top_level_keyword_span(body, 0, keyword)
        .map(|(position, end)| (&body[..position], &body[end..]))
}
