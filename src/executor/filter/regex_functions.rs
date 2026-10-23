//! Bounded PostgreSQL regular-expression scalar functions.

use regex_automata::{meta::Regex, util::syntax, Anchored, Input, Match, MatchKind};

use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::sql::functions::FunctionId;
use crate::types::Value;

use super::QueryError;

pub(super) fn evaluate(
    function: FunctionId,
    arguments: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    match function {
        FunctionId::RegexpLike => Some(regexp_like(arguments, controls)),
        FunctionId::RegexpCount => Some(regexp_count(arguments, controls)),
        _ => None,
    }
}

fn regexp_like(
    arguments: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    if arguments.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let input = text_argument("regexp_like", arguments, 0)?;
    let pattern = text_argument("regexp_like", arguments, 1)?;
    let flags = optional_text_argument("regexp_like", arguments, 2)?.unwrap_or_default();
    check_interruption(controls)?;
    let regex = compile(pattern, flags)?;
    check_interruption(controls)?;
    let matched = regex.is_match(input);
    check_interruption(controls)?;
    Ok(Value::Bool(matched))
}

fn regexp_count(
    arguments: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    if arguments.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let input = text_argument("regexp_count", arguments, 0)?;
    let pattern = text_argument("regexp_count", arguments, 1)?;
    let start = optional_integer_argument("regexp_count", arguments, 2)?.unwrap_or(1);
    let flags = optional_text_argument("regexp_count", arguments, 3)?.unwrap_or_default();
    if start < 1 {
        return Err(CassieError::InvalidParameterValue(
            "regexp_count start must be greater than zero".into(),
        )
        .into());
    }
    check_interruption(controls)?;
    let regex = compile(pattern, flags)?;
    let longest_regex = compile_with_match_kind(pattern, flags, Some(MatchKind::All))?;
    check_interruption(controls)?;
    let start = usize::try_from(start - 1).unwrap_or(usize::MAX);
    let byte_start = if let Some((byte, _)) = input.char_indices().nth(start) {
        byte
    } else if start == input.chars().count() {
        input.len()
    } else {
        return Ok(Value::Int64(0));
    };
    let mut count = 0_i32;
    let mut search_start = byte_start;
    loop {
        check_interruption(controls)?;
        let Some(matched) = find_leftmost_longest(&regex, &longest_regex, input, search_start)
        else {
            break;
        };
        increment_match_count(&mut count)?;
        search_start = if matched.is_empty() {
            let Some(character) = input[matched.end()..].chars().next() else {
                break;
            };
            matched.end() + character.len_utf8()
        } else {
            matched.end()
        };
    }
    check_interruption(controls)?;
    Ok(Value::Int64(i64::from(count)))
}

fn increment_match_count(count: &mut i32) -> Result<(), QueryError> {
    *count = count
        .checked_add(1)
        .ok_or_else(|| CassieError::Execution("integer out of range".into()))?;
    Ok(())
}

fn compile(pattern: &str, flags: &str) -> Result<Regex, QueryError> {
    compile_with_match_kind(pattern, flags, None)
}

fn compile_with_match_kind(
    pattern: &str,
    flags: &str,
    match_kind: Option<MatchKind>,
) -> Result<Regex, QueryError> {
    let options = RegexOptions::parse(flags)?;
    let pattern = if let Some(literal) = pattern.strip_prefix("***=") {
        escape(literal)
    } else {
        let pattern = pattern.strip_prefix("***:").unwrap_or(pattern);
        match options.syntax {
            PatternSyntax::Quoted => escape(pattern),
            PatternSyntax::Basic => basic_to_extended(pattern),
            PatternSyntax::Extended => pattern.to_owned(),
        }
    };
    let syntax = syntax::Config::new()
        .case_insensitive(matches!(options.case, CaseMode::Insensitive))
        .multi_line(options.newlines.multi_line())
        .dot_matches_new_line(options.newlines.dot_matches_new_line())
        .ignore_whitespace(matches!(options.whitespace, WhitespaceMode::Expanded));
    let mut builder = Regex::builder();
    builder.syntax(syntax);
    if let Some(match_kind) = match_kind {
        builder.configure(Regex::config().match_kind(match_kind));
    }
    builder
        .build(&pattern)
        .map_err(|error| invalid_regular_expression(&error.to_string()))
}

fn find_leftmost_longest(
    leftmost: &Regex,
    longest: &Regex,
    input: &str,
    search_start: usize,
) -> Option<Match> {
    let first = leftmost.find(Input::new(input).range(search_start..input.len()))?;
    longest.find(
        Input::new(input)
            .range(first.start()..input.len())
            .anchored(Anchored::Yes),
    )
}

fn text_argument<'a>(
    function: &str,
    arguments: &'a [Value],
    index: usize,
) -> Result<&'a str, QueryError> {
    match arguments.get(index) {
        Some(Value::String(value)) => Ok(value),
        _ => Err(
            CassieError::Execution(format!("function {function}(integer) does not exist")).into(),
        ),
    }
}

fn optional_text_argument<'a>(
    function: &str,
    arguments: &'a [Value],
    index: usize,
) -> Result<Option<&'a str>, QueryError> {
    arguments
        .get(index)
        .map(|value| match value {
            Value::String(value) => Ok(value.as_str()),
            _ => Err(CassieError::Execution(format!(
                "function {function}(integer) does not exist"
            ))
            .into()),
        })
        .transpose()
}

fn optional_integer_argument(
    function: &str,
    arguments: &[Value],
    index: usize,
) -> Result<Option<i64>, QueryError> {
    arguments
        .get(index)
        .map(|value| match value {
            Value::Int64(value) => Ok(*value),
            _ => Err(CassieError::Execution(format!(
                "function {function}(text, text, integer) does not exist"
            ))
            .into()),
        })
        .transpose()
}

fn check_interruption(controls: Option<&QueryExecutionControls>) -> Result<(), QueryError> {
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct RegexOptions {
    case: CaseMode,
    newlines: NewlineMode,
    whitespace: WhitespaceMode,
    syntax: PatternSyntax,
}

impl RegexOptions {
    fn parse(flags: &str) -> Result<Self, QueryError> {
        let mut options = Self {
            case: CaseMode::Sensitive,
            newlines: NewlineMode::Default,
            whitespace: WhitespaceMode::Tight,
            syntax: PatternSyntax::Extended,
        };
        for flag in flags.chars() {
            match flag {
                'b' => options.syntax = PatternSyntax::Basic,
                'e' => options.syntax = PatternSyntax::Extended,
                'q' => options.syntax = PatternSyntax::Quoted,
                'i' => options.case = CaseMode::Insensitive,
                'c' => options.case = CaseMode::Sensitive,
                'n' | 'm' => options.newlines = NewlineMode::NewlineSensitive,
                'p' => options.newlines = NewlineMode::PartialNewlineSensitive,
                'w' => options.newlines = NewlineMode::InversePartialNewlineSensitive,
                's' => options.newlines = NewlineMode::NonNewlineSensitive,
                't' => options.whitespace = WhitespaceMode::Tight,
                'x' => options.whitespace = WhitespaceMode::Expanded,
                _ => {
                    return Err(invalid_regular_expression(&format!(
                        "invalid regular expression flag '{flag}'"
                    )));
                }
            }
        }
        Ok(options)
    }
}

#[derive(Clone, Copy)]
enum PatternSyntax {
    Basic,
    Extended,
    Quoted,
}

#[derive(Clone, Copy)]
enum CaseMode {
    Sensitive,
    Insensitive,
}

#[derive(Clone, Copy)]
enum NewlineMode {
    Default,
    NewlineSensitive,
    PartialNewlineSensitive,
    InversePartialNewlineSensitive,
    NonNewlineSensitive,
}

impl NewlineMode {
    const fn multi_line(self) -> bool {
        matches!(
            self,
            Self::NewlineSensitive | Self::InversePartialNewlineSensitive
        )
    }

    const fn dot_matches_new_line(self) -> bool {
        matches!(
            self,
            Self::InversePartialNewlineSensitive | Self::NonNewlineSensitive
        )
    }
}

#[derive(Clone, Copy)]
enum WhitespaceMode {
    Tight,
    Expanded,
}

fn escape(pattern: &str) -> String {
    let mut escaped = String::with_capacity(pattern.len());
    for character in pattern.chars() {
        if matches!(
            character,
            '\\' | '.' | '^' | '$' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn basic_to_extended(pattern: &str) -> String {
    let mut converted = String::with_capacity(pattern.len());
    let mut characters = pattern.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            if let Some(next) = characters.next() {
                if matches!(next, '+' | '?' | '|' | '(' | ')' | '{' | '}') {
                    converted.push(next);
                } else {
                    converted.push('\\');
                    converted.push(next);
                }
            } else {
                converted.push('\\');
            }
        } else {
            if matches!(character, '+' | '?' | '|' | '(' | ')' | '{' | '}') {
                converted.push('\\');
            }
            converted.push(character);
        }
    }
    converted
}

fn invalid_regular_expression(message: &str) -> QueryError {
    CassieError::Diagnostic(Box::new(crate::app::DiagnosticError::new(
        "2201B",
        format!("invalid regular expression: {message}"),
    )))
    .into()
}

#[cfg(test)]
mod tests {
    use super::{
        compile, compile_with_match_kind, find_leftmost_longest, increment_match_count,
        regexp_count, regexp_like,
    };
    use crate::executor::filter::QueryError;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use crate::types::Value;
    use regex_automata::MatchKind;

    fn cancelled_controls() -> (QueryExecutionControls, QueryCancellationHandle) {
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
            cancellation.clone(),
        );
        (controls, cancellation)
    }

    #[test]
    fn should_find_leftmost_longest_alternatives_at_each_search_position() {
        // Arrange
        let input = "aaa";
        let leftmost = compile("a|aa", "").expect("leftmost regex compiles");
        let longest = compile_with_match_kind("a|aa", "", Some(MatchKind::All))
            .expect("longest regex compiles");

        // Act
        let first = find_leftmost_longest(&leftmost, &longest, input, 0).expect("first match");
        let second =
            find_leftmost_longest(&leftmost, &longest, input, first.end()).expect("second match");

        // Assert
        assert_eq!(second.start()..second.end(), 2..3);
    }

    #[test]
    fn should_report_22003_when_regexp_count_would_exceed_int4() {
        // Arrange
        let mut count = i32::MAX - 1;

        // Act
        increment_match_count(&mut count).expect("last valid INT4 count");
        let error = increment_match_count(&mut count).expect_err("INT4 overflow");

        // Assert
        assert_eq!(count, i32::MAX);
        let QueryError::Cassie(error) = error else {
            panic!("INT4 overflow should retain the Cassie numeric range error");
        };
        assert_eq!(error.descriptor().sql_state, "22003");
    }

    #[test]
    fn should_stop_regexp_like_before_compiling_when_query_is_cancelled() {
        // Arrange
        let (controls, cancellation) = cancelled_controls();
        cancellation.cancel();
        let arguments = [
            Value::String("value".to_owned()),
            Value::String("(".to_owned()),
        ];

        // Act
        let error = regexp_like(&arguments, Some(&controls)).expect_err("query is cancelled");

        // Assert
        let QueryError::Cassie(error) = error else {
            panic!("cancellation should remain a Cassie query error");
        };
        assert_eq!(error.descriptor().sql_state, "57014");
    }

    #[test]
    fn should_stop_regexp_count_before_compiling_when_query_is_cancelled() {
        // Arrange
        let (controls, cancellation) = cancelled_controls();
        cancellation.cancel();
        let arguments = [
            Value::String("value".to_owned()),
            Value::String("(".to_owned()),
        ];

        // Act
        let error = regexp_count(&arguments, Some(&controls)).expect_err("query is cancelled");

        // Assert
        let QueryError::Cassie(error) = error else {
            panic!("cancellation should remain a Cassie query error");
        };
        assert_eq!(error.descriptor().sql_state, "57014");
    }
}
