//! PostgreSQL regex flags and pattern preparation for the admitted native engine.

use regex_automata::{util::syntax, MatchKind};

use crate::runtime::QueryExecutionControls;

use super::{bounded::Regex, invalid_regular_expression, newline_classes, QueryError};

pub(super) fn compile(
    pattern: &str,
    flags: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<Regex, QueryError> {
    compile_with_match_kind(pattern, flags, None, controls)
}

pub(super) fn compile_with_match_kind(
    pattern: &str,
    flags: &str,
    match_kind: Option<MatchKind>,
    controls: Option<&QueryExecutionControls>,
) -> Result<Regex, QueryError> {
    let options = RegexOptions::parse(flags, false)?;
    compile_with_options(pattern, options, match_kind, controls)
}

pub(super) fn compile_for_replace(
    pattern: &str,
    flags: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<(Regex, Regex, bool), QueryError> {
    let options = RegexOptions::parse(flags, true)?;
    let leftmost = compile_with_options(pattern, options, None, controls)?;
    let longest = compile_with_options(pattern, options, Some(MatchKind::All), controls)?;
    Ok((leftmost, longest, options.global))
}

fn compile_with_options(
    pattern: &str,
    options: RegexOptions,
    match_kind: Option<MatchKind>,
    controls: Option<&QueryExecutionControls>,
) -> Result<Regex, QueryError> {
    let (pattern, quoted) = if matches!(options.syntax, PatternSyntax::Quoted) {
        (escape(pattern), true)
    } else if let Some(literal) = pattern.strip_prefix("***=") {
        (escape(literal), true)
    } else if let Some(advanced) = pattern.strip_prefix("***:") {
        (advanced.to_owned(), false)
    } else {
        let pattern = match options.syntax {
            PatternSyntax::Basic => basic_to_extended(pattern),
            PatternSyntax::Extended | PatternSyntax::Quoted => pattern.to_owned(),
        };
        (pattern, false)
    };
    let expanded = !quoted && matches!(options.whitespace, WhitespaceMode::Expanded);
    let corrected_classes = !quoted && options.newlines.excludes_class_newline();
    let pattern = if corrected_classes {
        newline_classes::exclude_newlines(&pattern, expanded)?
    } else {
        pattern
    };
    let mut syntax = syntax::Config::new()
        .case_insensitive(matches!(options.case, CaseMode::Insensitive))
        .multi_line(options.newlines.multi_line())
        .dot_matches_new_line(options.newlines.dot_matches_new_line())
        .ignore_whitespace(expanded);
    if corrected_classes {
        // The original AST already passed the default nesting limit. Each
        // negated class adds one bracket node and one intersection node to
        // the same path, which the backend's nesting visitor counts separately.
        syntax = syntax.nest_limit(syntax.get_nest_limit().saturating_mul(3));
    }
    Regex::build(&pattern, syntax, match_kind, controls)
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
        if matches!(options.syntax, PatternSyntax::Quoted)
            && (options.newlines.multi_line()
                || options.newlines.excludes_class_newline()
                || matches!(options.whitespace, WhitespaceMode::Expanded))
        {
            return Err(invalid_regular_expression(
                "invalid argument to regex function",
            ));
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
            Self::Default | Self::InversePartialNewlineSensitive | Self::NonNewlineSensitive
        )
    }

    const fn excludes_class_newline(self) -> bool {
        matches!(self, Self::NewlineSensitive | Self::PartialNewlineSensitive)
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
