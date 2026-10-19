//! `PREPARE`, `EXECUTE`, `DEALLOCATE`, `DECLARE`, `FETCH`, `MOVE`, `CLOSE`
//! and `DISCARD`, following the PostgreSQL 18 grammar.

use crate::sql::ast::{
    DeclareCursorStatement, DiscardTarget, ExecuteStatement, FetchDirection, FetchStatement,
    ParsedStatement, PrepareStatement, QueryStatement, SessionObjectStatement,
};
use crate::sql::parser::{expr::split_csv, lexical, parse_statement, SqlError};

/// Leading keywords of the statements this module parses.
pub(in crate::sql::parser) const SESSION_OBJECT_KEYWORDS: &[&str] = &[
    "prepare",
    "execute",
    "deallocate",
    "declare",
    "fetch",
    "move",
    "close",
    "discard",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// An unquoted word, lowercased.
    Word(String),
    /// A delimited identifier, unquoted.
    Quoted(String),
    /// A signed integer.
    Integer(String),
    Symbol(char),
}

struct Scanner<'a> {
    sql: &'a str,
    position: usize,
}

impl<'a> Scanner<'a> {
    const fn new(sql: &'a str, position: usize) -> Self {
        Self { sql, position }
    }

    fn rest(&self) -> &'a str {
        &self.sql[self.position..]
    }

    fn peek(&self) -> Result<Option<(Token, usize)>, SqlError> {
        let start = lexical::separator_end(self.sql, self.position);
        let Some(first) = self.sql[start..].chars().next() else {
            return Ok(None);
        };
        if first == '"' {
            let end = lexical::quoted_end(self.sql, start)
                .filter(|end| *end > start + 1 && self.sql[..*end].ends_with('"'))
                .ok_or_else(|| SqlError::syntax("unterminated quoted identifier".into()))?;
            let name = self.sql[start + 1..end - 1].replace("\"\"", "\"");
            return Ok(Some((Token::Quoted(name), end)));
        }
        let signed = matches!(first, '+' | '-')
            && self.sql[start + 1..]
                .chars()
                .next()
                .is_some_and(|next| next.is_ascii_digit());
        if first.is_ascii_digit() || signed {
            let digits_start = start + usize::from(signed);
            let end = self.sql[digits_start..]
                .find(|character: char| !character.is_ascii_digit())
                .map_or(self.sql.len(), |offset| digits_start + offset);
            return Ok(Some((
                Token::Integer(self.sql[start..end].to_string()),
                end,
            )));
        }
        if lexical::identifier_character(first) {
            let end = self.sql[start..]
                .find(|character: char| !lexical::identifier_character(character))
                .map_or(self.sql.len(), |offset| start + offset);
            return Ok(Some((
                Token::Word(self.sql[start..end].to_ascii_lowercase()),
                end,
            )));
        }
        Ok(Some((Token::Symbol(first), start + first.len_utf8())))
    }

    fn next(&mut self) -> Result<Option<Token>, SqlError> {
        let Some((token, end)) = self.peek()? else {
            return Ok(None);
        };
        self.position = end;
        Ok(Some(token))
    }

    fn eat_word(&mut self, word: &str) -> Result<bool, SqlError> {
        if matches!(self.peek()?, Some((Token::Word(found), _)) if found == word) {
            self.next()?;
            return Ok(true);
        }
        Ok(false)
    }

    fn expect_word(&mut self, word: &str) -> Result<(), SqlError> {
        if self.eat_word(word)? {
            Ok(())
        } else {
            Err(syntax_near(self))
        }
    }

    fn name(&mut self) -> Result<String, SqlError> {
        match self.next()? {
            Some(Token::Word(name) | Token::Quoted(name)) => Ok(name),
            _ => Err(SqlError::syntax("syntax error: expected a name".into())),
        }
    }

    fn finish(&self) -> Result<(), SqlError> {
        if self.peek()?.is_some() {
            return Err(syntax_near(self));
        }
        Ok(())
    }
}

fn syntax_near(scanner: &Scanner<'_>) -> SqlError {
    let rest = lexical::trim_separators(scanner.rest());
    let near = rest.split_whitespace().next().unwrap_or("end of input");
    SqlError::syntax(format!("syntax error at or near \"{near}\""))
}

/// Parses one session-object statement; `sql` starts with its keyword.
pub(in crate::sql::parser) fn parse_session_object_statement(
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    let sql = sql.trim().trim_end_matches(';').trim();
    let mut scanner = Scanner::new(sql, 0);
    let Some(Token::Word(keyword)) = scanner.next()? else {
        return Err(SqlError::syntax("expected a statement keyword".into()));
    };
    let statement = match keyword.as_str() {
        "prepare" => parse_prepare(&mut scanner)?,
        "execute" => parse_execute(&mut scanner)?,
        "deallocate" => {
            scanner.eat_word("prepare")?;
            let name = scanner.name()?;
            scanner.finish()?;
            SessionObjectStatement::Deallocate((name != "all").then_some(name))
        }
        "declare" => parse_declare(&mut scanner)?,
        "fetch" | "move" => parse_fetch(&mut scanner, keyword == "move")?,
        "close" => {
            let name = scanner.name()?;
            scanner.finish()?;
            SessionObjectStatement::CloseCursor((name != "all").then_some(name))
        }
        "discard" => parse_discard(&mut scanner)?,
        _ => return Err(SqlError::syntax("unsupported session statement".into())),
    };
    Ok(ParsedStatement {
        raw_sql: sql.to_string(),
        statement: QueryStatement::SessionObject(statement),
    })
}

fn parse_discard(scanner: &mut Scanner<'_>) -> Result<SessionObjectStatement, SqlError> {
    let target = match scanner.next()? {
        Some(Token::Word(word)) => match word.as_str() {
            "all" => DiscardTarget::All,
            "plans" => DiscardTarget::Plans,
            "sequences" => DiscardTarget::Sequences,
            "temp" | "temporary" => DiscardTarget::Temp,
            _ => {
                return Err(SqlError::syntax(format!(
                    "syntax error at or near \"{word}\""
                )))
            }
        },
        _ => return Err(SqlError::syntax("DISCARD requires a target".into())),
    };
    scanner.finish()?;
    Ok(SessionObjectStatement::Discard(target))
}

fn parse_prepare(scanner: &mut Scanner<'_>) -> Result<SessionObjectStatement, SqlError> {
    let name = scanner.name()?;
    let mut parameter_types = Vec::new();
    if matches!(scanner.peek()?, Some((Token::Symbol('('), _))) {
        let open = lexical::separator_end(scanner.sql, scanner.position);
        let close = lexical::matching_paren(scanner.sql, open)
            .ok_or_else(|| SqlError::syntax("unterminated PREPARE parameter list".into()))?;
        let list = scanner.sql[open + 1..close].trim();
        if !list.is_empty() {
            for parameter_type in split_csv(list) {
                let parameter_type = parameter_type.trim();
                if parameter_type.is_empty() {
                    return Err(SqlError::syntax("empty PREPARE parameter type".into()));
                }
                parameter_types.push(parameter_type.to_string());
            }
        }
        scanner.position = close + 1;
    }
    scanner.expect_word("as")?;
    let statement_sql = lexical::trim_separators(scanner.rest()).to_string();
    let statement = parse_statement(&statement_sql)?;
    if !matches!(
        statement.statement,
        QueryStatement::Select(_)
            | QueryStatement::Insert(_)
            | QueryStatement::Update(_)
            | QueryStatement::Delete(_)
    ) {
        return Err(SqlError::syntax(format!(
            "syntax error at or near \"{}\"",
            statement_sql.split_whitespace().next().unwrap_or_default()
        )));
    }
    Ok(SessionObjectStatement::Prepare(PrepareStatement {
        name,
        parameter_types,
        statement_sql,
        statement: Box::new(statement),
    }))
}

fn parse_execute(scanner: &mut Scanner<'_>) -> Result<SessionObjectStatement, SqlError> {
    let name = scanner.name()?;
    let mut arguments = Vec::new();
    if matches!(scanner.peek()?, Some((Token::Symbol('('), _))) {
        let open = lexical::separator_end(scanner.sql, scanner.position);
        let close = lexical::matching_paren(scanner.sql, open)
            .ok_or_else(|| SqlError::syntax("unterminated EXECUTE argument list".into()))?;
        let list = scanner.sql[open + 1..close].trim();
        if !list.is_empty() {
            for argument in split_csv(list) {
                let argument = argument.trim();
                if argument.is_empty() {
                    return Err(SqlError::syntax("empty EXECUTE argument".into()));
                }
                arguments.push(argument.to_string());
            }
        }
        scanner.position = close + 1;
    }
    scanner.finish()?;
    Ok(SessionObjectStatement::Execute(ExecuteStatement {
        name,
        arguments,
    }))
}

fn parse_declare(scanner: &mut Scanner<'_>) -> Result<SessionObjectStatement, SqlError> {
    let name = scanner.name()?;
    let (mut binary, mut scroll, mut sensitivity) = (false, None, None);
    loop {
        let Some(Token::Word(option)) = scanner.next()? else {
            return Err(syntax_near(scanner));
        };
        match option.as_str() {
            "cursor" => break,
            "binary" => binary = true,
            "scroll" | "no" => {
                let requested = option == "scroll" || {
                    scanner.expect_word("scroll")?;
                    false
                };
                if scroll.is_some_and(|current| current != requested) {
                    return Err(SqlError::syntax(
                        "cannot specify both SCROLL and NO SCROLL".into(),
                    ));
                }
                scroll = Some(requested);
            }
            "insensitive" | "asensitive" => {
                if sensitivity.is_some_and(|current: &str| current != option) {
                    return Err(SqlError::syntax(
                        "cannot specify both INSENSITIVE and ASENSITIVE".into(),
                    ));
                }
                sensitivity = Some(if option == "insensitive" {
                    "insensitive"
                } else {
                    "asensitive"
                });
            }
            _ => {
                return Err(SqlError::syntax(format!(
                    "syntax error at or near \"{option}\""
                )))
            }
        }
    }
    let hold = if scanner.eat_word("with")? {
        scanner.expect_word("hold")?;
        true
    } else {
        if scanner.eat_word("without")? {
            scanner.expect_word("hold")?;
        }
        false
    };
    scanner.expect_word("for")?;
    let query_sql = lexical::trim_separators(scanner.rest()).to_string();
    let query = parse_statement(&query_sql)?;
    if !matches!(query.statement, QueryStatement::Select(_)) {
        return Err(SqlError::syntax(format!(
            "syntax error at or near \"{}\"",
            query_sql.split_whitespace().next().unwrap_or_default()
        )));
    }
    Ok(SessionObjectStatement::DeclareCursor(
        DeclareCursorStatement {
            name,
            binary,
            scroll,
            hold,
            query_sql,
            query: Box::new(query),
        },
    ))
}

fn parse_count(token: Option<Token>) -> Result<i64, SqlError> {
    match token {
        // PostgreSQL's grammar admits a 32-bit signed count.
        Some(Token::Integer(digits)) => digits
            .parse::<i32>()
            .map(i64::from)
            .map_err(|_| SqlError::syntax(format!("syntax error at or near \"{digits}\""))),
        _ => Err(SqlError::syntax("FETCH requires a row count".into())),
    }
}

fn signed_forward(count: i64) -> FetchDirection {
    if count < 0 {
        FetchDirection::Backward(Some(-count))
    } else {
        FetchDirection::Forward(Some(count))
    }
}

fn parse_fetch(
    scanner: &mut Scanner<'_>,
    move_only: bool,
) -> Result<SessionObjectStatement, SqlError> {
    let mut tokens = Vec::new();
    while let Some(token) = scanner.next()? {
        tokens.push(token);
    }
    let Some(Token::Word(cursor) | Token::Quoted(cursor)) = tokens.pop() else {
        return Err(SqlError::syntax(
            "FETCH and MOVE require a cursor name".into(),
        ));
    };
    if matches!(tokens.last(), Some(Token::Word(word)) if word == "from" || word == "in") {
        tokens.pop();
    }
    let mut tokens = tokens.into_iter();
    let first = tokens.next();
    let direction = match first {
        None => FetchDirection::Forward(Some(1)),
        Some(Token::Integer(_)) => signed_forward(parse_count(first)?),
        Some(Token::Word(word)) => match word.as_str() {
            "next" => FetchDirection::Forward(Some(1)),
            "prior" => FetchDirection::Backward(Some(1)),
            "first" => FetchDirection::Absolute(1),
            "last" => FetchDirection::Absolute(-1),
            "all" => FetchDirection::Forward(None),
            "absolute" => FetchDirection::Absolute(parse_count(tokens.next())?),
            "relative" => FetchDirection::Relative(parse_count(tokens.next())?),
            "forward" | "backward" => {
                let count = match tokens.next() {
                    None => Some(1),
                    Some(Token::Word(all)) if all == "all" => None,
                    other => Some(parse_count(other)?),
                };
                match (word == "forward", count) {
                    (true, Some(count)) => signed_forward(count),
                    (false, Some(count)) => signed_forward(-count),
                    (true, None) => FetchDirection::Forward(None),
                    (false, None) => FetchDirection::Backward(None),
                }
            }
            _ => {
                return Err(SqlError::syntax(format!(
                    "syntax error at or near \"{word}\""
                )))
            }
        },
        Some(_) => return Err(SqlError::syntax("invalid FETCH direction".into())),
    };
    if tokens.next().is_some() {
        return Err(SqlError::syntax("invalid FETCH direction".into()));
    }
    Ok(SessionObjectStatement::Fetch(FetchStatement {
        cursor,
        direction,
        move_only,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch(sql: &str) -> FetchStatement {
        match parse_session_object_statement(sql).map(|parsed| parsed.statement) {
            Ok(QueryStatement::SessionObject(SessionObjectStatement::Fetch(fetch))) => fetch,
            _ => panic!("expected FETCH for {sql}"),
        }
    }

    #[test]
    fn should_parse_every_fetch_direction_given_postgresql_forms() {
        // Arrange
        let inputs = [
            "FETCH c",
            "FETCH NEXT FROM c",
            "FETCH PRIOR IN c",
            "FETCH FIRST c",
            "FETCH LAST c",
            "FETCH ABSOLUTE -2 c",
            "FETCH RELATIVE 0 c",
            "FETCH -1 FROM c",
            "FETCH ALL c",
            "FETCH FORWARD ALL c",
            "FETCH BACKWARD ALL c",
            "FETCH BACKWARD -2 c",
            "MOVE FORWARD 3 IN \"MiXed\"",
        ];

        // Act
        let directions = inputs.map(|sql| fetch(sql).direction);

        // Assert
        assert_eq!(
            directions,
            [
                FetchDirection::Forward(Some(1)),
                FetchDirection::Forward(Some(1)),
                FetchDirection::Backward(Some(1)),
                FetchDirection::Absolute(1),
                FetchDirection::Absolute(-1),
                FetchDirection::Absolute(-2),
                FetchDirection::Relative(0),
                FetchDirection::Backward(Some(1)),
                FetchDirection::Forward(None),
                FetchDirection::Forward(None),
                FetchDirection::Backward(None),
                FetchDirection::Forward(Some(2)),
                FetchDirection::Forward(Some(3)),
            ]
        );
        assert_eq!(fetch("MOVE FORWARD 3 IN \"MiXed\"").cursor, "MiXed");
        assert!(fetch("MOVE 1 c").move_only);
    }

    #[test]
    fn should_reject_fetch_counts_outside_the_postgresql_grammar() {
        // Arrange
        let inputs = ["FETCH 3000000000 c", "FETCH c IN c", "FETCH"];

        // Act
        let rejected = inputs.map(|sql| parse_session_object_statement(sql).is_err());

        // Assert
        assert_eq!(rejected, [true, true, true]);
    }

    #[test]
    fn should_parse_declare_options_given_any_postgresql_order() {
        // Arrange
        let sql = "DECLARE c BINARY INSENSITIVE NO SCROLL CURSOR WITH HOLD FOR SELECT 1";

        // Act
        let parsed = parse_session_object_statement(sql).map(|parsed| parsed.statement);

        // Assert
        let Ok(QueryStatement::SessionObject(SessionObjectStatement::DeclareCursor(declare))) =
            parsed
        else {
            panic!("expected DECLARE");
        };
        assert_eq!(
            (
                declare.name.as_str(),
                declare.binary,
                declare.scroll,
                declare.hold
            ),
            ("c", true, Some(false), true)
        );
        assert_eq!(declare.query_sql, "SELECT 1");
    }

    #[test]
    fn should_reject_declare_given_conflicting_options_or_a_non_select_query() {
        // Arrange
        let inputs = [
            "DECLARE c SCROLL NO SCROLL CURSOR FOR SELECT 1",
            "DECLARE c INSENSITIVE ASENSITIVE CURSOR FOR SELECT 1",
            "DECLARE c CURSOR FOR INSERT INTO t VALUES (1)",
        ];

        // Act
        let rejected = inputs.map(|sql| parse_session_object_statement(sql).is_err());

        // Assert
        assert_eq!(rejected, [true, true, true]);
    }

    #[test]
    fn should_parse_typed_prepared_statement_given_declared_parameters() {
        // Arrange
        let prepare = "PREPARE p (INT, varchar(10)) AS SELECT $1, $2";
        let execute = "EXECUTE p (1 + 1, 'a, b')";

        // Act
        let prepared = parse_session_object_statement(prepare).map(|parsed| parsed.statement);
        let executed = parse_session_object_statement(execute).map(|parsed| parsed.statement);

        // Assert
        let Ok(QueryStatement::SessionObject(SessionObjectStatement::Prepare(prepared))) = prepared
        else {
            panic!("expected PREPARE");
        };
        assert_eq!(prepared.parameter_types, ["INT", "varchar(10)"]);
        assert_eq!(prepared.statement_sql, "SELECT $1, $2");
        let Ok(QueryStatement::SessionObject(SessionObjectStatement::Execute(executed))) = executed
        else {
            panic!("expected EXECUTE");
        };
        assert_eq!(executed.arguments, ["1 + 1", "'a, b'"]);
    }
}
