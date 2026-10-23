//! Bounded PostgreSQL regular-expression scalar functions.

use regex_automata::{
    meta::Regex,
    util::{captures::Captures, syntax},
    Anchored, Input, Match, MatchKind,
};

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
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
        FunctionId::RegexpReplace => Some(regexp_replace(arguments, controls)),
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

fn regexp_replace(
    arguments: &[Value],
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    if arguments.iter().any(|value| matches!(value, Value::Null)) {
        return Ok(Value::Null);
    }
    let input = text_argument("regexp_replace", arguments, 0)?;
    let pattern = text_argument("regexp_replace", arguments, 1)?;
    let replacement = text_argument("regexp_replace", arguments, 2)?;
    let (start, occurrence, flags, global_default) = match arguments {
        [_, _, _] => (1, 1, "", false),
        [_, _, _, Value::String(flags)] => (1, 1, flags.as_str(), true),
        [_, _, _, Value::Int64(start)] => (*start, 1, "", false),
        [_, _, _, Value::Int64(start), Value::Int64(occurrence)] => {
            (*start, *occurrence, "", false)
        }
        [_, _, _, Value::Int64(start), Value::Int64(occurrence), Value::String(flags)] => {
            (*start, *occurrence, flags.as_str(), false)
        }
        _ => {
            return Err(CassieError::Execution(
                "function regexp_replace has no matching argument signature".into(),
            )
            .into());
        }
    };
    if start < 1 {
        return Err(CassieError::InvalidParameterValue(
            "regexp_replace start must be greater than zero".into(),
        )
        .into());
    }
    if occurrence < 0 {
        return Err(CassieError::InvalidParameterValue(
            "regexp_replace occurrence must not be negative".into(),
        )
        .into());
    }

    check_interruption(controls)?;
    let (leftmost, longest, global_flag) = compile_for_replace(pattern, flags)?;
    check_interruption(controls)?;
    let Some(start_byte) = character_start(input, start, controls)? else {
        return copy_input(input, controls);
    };
    let replace_all = occurrence == 0 || global_default && global_flag;
    let occurrence = usize::try_from(occurrence).unwrap_or(usize::MAX);
    let matches = ReplacementWalk {
        input,
        leftmost: &leftmost,
        longest: &longest,
        start: start_byte,
        occurrence,
        replace_all,
    };
    let mut check = || check_interruption(controls);

    let mut output_length = 0_usize;
    let cursor = matches.walk(&mut check, |captures, unmatched| {
        output_length = output_length
            .checked_add(unmatched.len())
            .ok_or_else(|| resource_limit("regexp_replace output size overflow"))?;
        output_length = output_length
            .checked_add(replacement_length(replacement, captures, controls)?)
            .ok_or_else(|| resource_limit("regexp_replace output size overflow"))?;
        Ok(())
    })?;
    output_length = output_length
        .checked_add(input.len() - cursor)
        .ok_or_else(|| resource_limit("regexp_replace output size overflow"))?;
    let _memory = reserve_output(controls, output_length)?;
    let mut output = output_string(output_length)?;
    let cursor = matches.walk(&mut check, |captures, unmatched| {
        check_interruption(controls)?;
        output.push_str(&input[unmatched]);
        append_replacement(&mut output, replacement, input, captures, controls)
    })?;
    output.push_str(&input[cursor..]);
    check_interruption(controls)?;
    Ok(Value::String(output))
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
    let options = RegexOptions::parse(flags, false)?;
    compile_with_options(pattern, options, match_kind)
}

fn compile_for_replace(pattern: &str, flags: &str) -> Result<(Regex, Regex, bool), QueryError> {
    let options = RegexOptions::parse(flags, true)?;
    let leftmost = compile_with_options(pattern, options, None)?;
    let longest = compile_with_options(pattern, options, Some(MatchKind::All))?;
    Ok((leftmost, longest, options.global))
}

fn compile_with_options(
    pattern: &str,
    options: RegexOptions,
    match_kind: Option<MatchKind>,
) -> Result<Regex, QueryError> {
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

struct ReplacementWalk<'a> {
    input: &'a str,
    leftmost: &'a Regex,
    longest: &'a Regex,
    start: usize,
    occurrence: usize,
    replace_all: bool,
}

impl ReplacementWalk<'_> {
    fn walk(
        &self,
        check: &mut impl FnMut() -> Result<(), QueryError>,
        mut visit: impl FnMut(&Captures, std::ops::Range<usize>) -> Result<(), QueryError>,
    ) -> Result<usize, QueryError> {
        let mut captures = self.longest.create_captures();
        let mut search_start = self.start;
        let mut output_cursor = 0_usize;
        let mut found = 0_usize;
        loop {
            check()?;
            let Some(matched) = find_leftmost_longest_captures(
                self.leftmost,
                self.longest,
                self.input,
                search_start,
                &mut captures,
            ) else {
                break;
            };
            found = found
                .checked_add(1)
                .ok_or_else(|| resource_limit("regexp_replace match count overflow"))?;
            if self.replace_all || found == self.occurrence {
                visit(&captures, output_cursor..matched.start())?;
                output_cursor = matched.end();
                if !self.replace_all {
                    break;
                }
            }
            let Some(next_start) = advance_after_match(self.input, matched) else {
                break;
            };
            search_start = next_start;
        }
        Ok(output_cursor)
    }
}

fn find_leftmost_longest_captures(
    leftmost: &Regex,
    longest: &Regex,
    input: &str,
    search_start: usize,
    captures: &mut Captures,
) -> Option<Match> {
    let first = leftmost.find(Input::new(input).range(search_start..input.len()))?;
    longest.search_captures(
        &Input::new(input)
            .range(first.start()..input.len())
            .anchored(Anchored::Yes),
        captures,
    );
    captures.get_match()
}

fn advance_after_match(input: &str, matched: Match) -> Option<usize> {
    if matched.end() > matched.start() {
        Some(matched.end())
    } else {
        input[matched.end()..]
            .chars()
            .next()
            .map(|character| matched.end() + character.len_utf8())
    }
}

fn character_start(
    input: &str,
    start: i64,
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<usize>, QueryError> {
    let requested = usize::try_from(start - 1).unwrap_or(usize::MAX);
    let mut character_count = 0_usize;
    for (byte, _) in input.char_indices() {
        if character_count.is_multiple_of(1024) {
            check_interruption(controls)?;
        }
        if character_count == requested {
            return Ok(Some(byte));
        }
        character_count += 1;
    }
    check_interruption(controls)?;
    Ok((character_count == requested).then_some(input.len()))
}

fn replacement_length(
    replacement: &str,
    captures: &Captures,
    controls: Option<&QueryExecutionControls>,
) -> Result<usize, QueryError> {
    let mut length = 0_usize;
    let mut offset = 0_usize;
    let mut steps = 0_usize;
    while offset < replacement.len() {
        if steps.is_multiple_of(1024) {
            check_interruption(controls)?;
        }
        steps += 1;
        if replacement.as_bytes()[offset] != b'\\' {
            let character = replacement[offset..]
                .chars()
                .next()
                .expect("offset remains within replacement");
            offset += character.len_utf8();
            length = checked_output_length(length, character.len_utf8())?;
            continue;
        }
        offset += 1;
        let Some(character) = replacement[offset..].chars().next() else {
            length = checked_output_length(length, 1)?;
            break;
        };
        offset += character.len_utf8();
        let added = match character {
            '1'..='9' => captures
                .get_group(replacement_group_index(character))
                .map_or(0, |span| span.end - span.start),
            '&' => captures
                .get_match()
                .map_or(0, |matched| matched.end() - matched.start()),
            '\\' => 1,
            _ => 1 + character.len_utf8(),
        };
        length = checked_output_length(length, added)?;
    }
    Ok(length)
}

fn append_replacement(
    output: &mut String,
    replacement: &str,
    input: &str,
    captures: &Captures,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    let mut offset = 0_usize;
    let mut steps = 0_usize;
    while offset < replacement.len() {
        if steps.is_multiple_of(1024) {
            check_interruption(controls)?;
        }
        steps += 1;
        if replacement.as_bytes()[offset] != b'\\' {
            let character = replacement[offset..]
                .chars()
                .next()
                .expect("offset remains within replacement");
            output.push(character);
            offset += character.len_utf8();
            continue;
        }
        offset += 1;
        let Some(character) = replacement[offset..].chars().next() else {
            output.push('\\');
            break;
        };
        offset += character.len_utf8();
        match character {
            '1'..='9' => {
                if let Some(span) = captures.get_group(replacement_group_index(character)) {
                    output.push_str(&input[span.start..span.end]);
                }
            }
            '&' => {
                if let Some(matched) = captures.get_match() {
                    output.push_str(&input[matched.start()..matched.end()]);
                }
            }
            '\\' => output.push('\\'),
            _ => {
                output.push('\\');
                output.push(character);
            }
        }
    }
    Ok(())
}

fn replacement_group_index(character: char) -> usize {
    usize::try_from(u32::from(character) - u32::from('0'))
        .expect("single-digit capture group index fits usize")
}

fn checked_output_length(length: usize, addition: usize) -> Result<usize, QueryError> {
    length
        .checked_add(addition)
        .ok_or_else(|| resource_limit("regexp_replace output size overflow"))
}

fn copy_input(input: &str, controls: Option<&QueryExecutionControls>) -> Result<Value, QueryError> {
    check_interruption(controls)?;
    let _memory = reserve_output(controls, input.len())?;
    let mut output = output_string(input.len())?;
    output.push_str(input);
    check_interruption(controls)?;
    Ok(Value::String(output))
}

fn reserve_output(
    controls: Option<&QueryExecutionControls>,
    bytes: usize,
) -> Result<Option<QueryMemoryReservation>, QueryError> {
    Ok(controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()?)
}

fn output_string(capacity: usize) -> Result<String, QueryError> {
    let mut output = String::new();
    output.try_reserve_exact(capacity).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to allocate regexp_replace output: {error}"))
    })?;
    Ok(output)
}

fn resource_limit(message: &str) -> QueryError {
    CassieError::ResourceLimit(message.to_owned()).into()
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
    global: bool,
}

impl RegexOptions {
    fn parse(flags: &str, allow_global: bool) -> Result<Self, QueryError> {
        let mut options = Self {
            case: CaseMode::Sensitive,
            newlines: NewlineMode::Default,
            whitespace: WhitespaceMode::Tight,
            syntax: PatternSyntax::Extended,
            global: false,
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
                'g' if allow_global => options.global = true,
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
        regexp_count, regexp_like, regexp_replace, ReplacementWalk,
    };
    use crate::app::CassieError;
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

    #[test]
    fn should_stop_regexp_replace_when_cancellation_arrives_during_match_iteration() {
        // Arrange
        let leftmost = compile("a", "").expect("leftmost regex compiles");
        let longest =
            compile_with_match_kind("a", "", Some(MatchKind::All)).expect("longest regex compiles");
        let mut checks = 0;
        let mut check = || {
            checks += 1;
            if checks == 3 {
                Err(CassieError::QueryCancelled.into())
            } else {
                Ok(())
            }
        };

        let matches = ReplacementWalk {
            input: "aaaa",
            leftmost: &leftmost,
            longest: &longest,
            start: 0,
            occurrence: 0,
            replace_all: true,
        };

        // Act
        let error = matches
            .walk(&mut check, |_, _| Ok(()))
            .expect_err("cancellation during iteration is observed");

        // Assert
        let QueryError::Cassie(error) = error else {
            panic!("cancellation should remain a Cassie query error");
        };
        assert_eq!(error.descriptor().sql_state, "57014");
    }

    #[test]
    fn should_stop_regexp_replace_before_compiling_when_query_is_cancelled() {
        // Arrange
        let (controls, cancellation) = cancelled_controls();
        cancellation.cancel();
        let arguments = [
            Value::String("value".to_owned()),
            Value::String("(".to_owned()),
            Value::String("replacement".to_owned()),
        ];

        // Act
        let error = regexp_replace(&arguments, Some(&controls)).expect_err("query is cancelled");

        // Assert
        let QueryError::Cassie(error) = error else {
            panic!("cancellation should remain a Cassie query error");
        };
        assert_eq!(error.descriptor().sql_state, "57014");
    }
}
