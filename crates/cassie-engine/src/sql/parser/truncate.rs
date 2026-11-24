//! `TRUNCATE [TABLE] [ONLY] name [*] [, ...] [RESTART | CONTINUE IDENTITY]
//! [CASCADE | RESTRICT]`.

use super::expr::split_csv;
use super::identifiers::parse_relation_path_prefix;
use super::lexical::{pattern_end, trim_separators};
use super::{ParsedStatement, QueryStatement, SqlError};
use crate::sql::ast::TruncateStatement;

pub(super) fn parse_truncate_statement(sql: &str) -> Result<ParsedStatement, SqlError> {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let end = pattern_end(trimmed, 0, "truncate")
        .ok_or_else(|| SqlError::new("TRUNCATE requires a table".into()))?;
    let mut rest = trim_separators(&trimmed[end..]);
    if let Some(end) = pattern_end(rest, 0, "table") {
        rest = trim_separators(&rest[end..]);
    }
    let (rest, cascade) = strip_trailing_options(rest)?;
    if rest.is_empty() {
        return Err(SqlError::syntax(
            "syntax error at end of input: TRUNCATE requires a table".into(),
        ));
    }
    let mut tables = Vec::new();
    for item in split_csv(rest) {
        let mut item = trim_separators(item);
        if let Some(end) = pattern_end(item, 0, "only") {
            item = trim_separators(&item[end..]);
        }
        let item = item.strip_suffix('*').map_or(item, str::trim_end);
        if item.is_empty() {
            return Err(SqlError::syntax(
                "TRUNCATE table list cannot include an empty name".into(),
            ));
        }
        let (table, tail) = parse_relation_path_prefix(item)?;
        if !trim_separators(tail).is_empty() || table.is_empty() {
            return Err(SqlError::syntax(format!(
                "syntax error at or near \"{}\"",
                trim_separators(tail)
            )));
        }
        tables.push(table);
    }
    Ok(ParsedStatement {
        raw_sql: trimmed.to_string(),
        statement: QueryStatement::Truncate(TruncateStatement {
            tables,
            restart_identity: cascade.restart_identity,
            cascade: cascade.cascade,
        }),
    })
}

#[derive(Default)]
struct TruncateOptions {
    restart_identity: bool,
    cascade: bool,
}

/// Strips `[RESTART | CONTINUE IDENTITY] [CASCADE | RESTRICT]` from the end.
fn strip_trailing_options(raw: &str) -> Result<(&str, TruncateOptions), SqlError> {
    let mut options = TruncateOptions::default();
    let mut rest = raw.trim_end();
    if let Some(head) = strip_keyword_suffix(rest, "cascade") {
        options.cascade = true;
        rest = head;
    } else if let Some(head) = strip_keyword_suffix(rest, "restrict") {
        rest = head;
    }
    if let Some(head) = strip_keyword_suffix(rest, "identity") {
        if let Some(head) = strip_keyword_suffix(head, "restart") {
            options.restart_identity = true;
            rest = head;
        } else if let Some(head) = strip_keyword_suffix(head, "continue") {
            rest = head;
        } else {
            return Err(SqlError::syntax(
                "syntax error at or near \"IDENTITY\"".into(),
            ));
        }
    }
    Ok((rest.trim_end(), options))
}

fn strip_keyword_suffix<'a>(raw: &'a str, keyword: &str) -> Option<&'a str> {
    super::lexical::strip_keyword_suffix(raw, keyword)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(sql: &str) -> TruncateStatement {
        match parse_truncate_statement(sql)
            .expect("parse TRUNCATE")
            .statement
        {
            QueryStatement::Truncate(statement) => statement,
            _ => panic!("expected a TRUNCATE statement"),
        }
    }

    #[test]
    fn should_parse_every_truncate_option_given_a_table_list() {
        // Arrange
        let sql = "TRUNCATE TABLE ONLY a, public.b * RESTART IDENTITY CASCADE";

        // Act
        let statement = parsed(sql);

        // Assert
        assert_eq!(
            statement
                .tables
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            vec!["a".to_string(), "public.b".to_string()]
        );
        assert!(statement.restart_identity);
        assert!(statement.cascade);
    }

    #[test]
    fn should_apply_the_default_truncate_options_given_a_bare_truncate() {
        // Arrange
        let sql = "truncate t CONTINUE IDENTITY RESTRICT";

        // Act
        let statement = parsed(sql);

        // Assert
        assert_eq!(statement.tables.len(), 1);
        assert!(!statement.restart_identity);
        assert!(!statement.cascade);
    }

    #[test]
    fn should_reject_truncate_without_a_table() {
        // Arrange
        let sql = "TRUNCATE TABLE";

        // Act
        let rejected = parse_truncate_statement(sql);

        // Assert
        assert!(rejected.is_err());
    }
}
