//! Source notation admission before the public AST erases lexical distinctions.
//!
//! Matching and structural validity belong to the published parser. This guard
//! preserves ignored contexts while rejecting unselected aliases and constraints
//! inside classes; it does not build an alternative matching AST.

use super::{check_interruption, invalid_regular_expression, memory::Directory, QueryError};
use crate::runtime::QueryExecutionControls;

struct Bracket {
    first: bool,
    negation: bool,
}

pub(super) fn validate(
    input: &str,
    expanded: bool,
    advanced: bool,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    let mut guard = Guard {
        input,
        offset: 0,
        expanded,
        advanced,
        brackets: Directory::new(),
        scopes: Directory::new(),
        controls,
    };
    guard.scan()
}

struct Guard<'a> {
    input: &'a str,
    offset: usize,
    expanded: bool,
    advanced: bool,
    brackets: Directory<Bracket>,
    scopes: Directory<bool>,
    controls: Option<&'a QueryExecutionControls>,
}

impl Guard<'_> {
    fn scan(&mut self) -> Result<(), QueryError> {
        while self.offset < self.input.len() {
            check_interruption(self.controls)?;
            let suffix = &self.input[self.offset..];
            if suffix.starts_with("[[:<:]]") || suffix.starts_with("[[:>:]]") {
                if !self.brackets.is_empty() {
                    return invalid("word constraint is not allowed in a bracket");
                }
                self.offset += 7;
                continue;
            }
            let character = suffix.chars().next().expect("source scalar boundary");
            if character == '\\' {
                self.escape()?;
                continue;
            }
            if self.brackets.is_empty() {
                if self.expanded && character == '#' {
                    self.line_comment()?;
                    continue;
                }
                if character == '(' {
                    self.group()?;
                    continue;
                }
                if character == ')' {
                    if let Some(expanded) = self.scopes.values.pop() {
                        self.expanded = expanded;
                    }
                }
            }
            self.bracket(character)?;
            self.offset += character.len_utf8();
        }
        Ok(())
    }

    fn escape(&mut self) -> Result<(), QueryError> {
        self.offset += 1;
        let Some(character) = self.input[self.offset..].chars().next() else {
            return Ok(());
        };
        if !character.is_ascii() {
            return invalid("unselected escaped non-ASCII source character");
        }
        if self.advanced
            && !self.brackets.is_empty()
            && matches!(character, 'm' | 'M' | 'y' | 'Y' | 'Z')
        {
            return invalid("word or anchor constraint is not allowed in a bracket");
        }
        if character.is_ascii_alphabetic()
            && !matches!(
                character,
                'A' | 'z'
                    | 'b'
                    | 'B'
                    | 'p'
                    | 'P'
                    | 'd'
                    | 'D'
                    | 's'
                    | 'S'
                    | 'w'
                    | 'W'
                    | 'a'
                    | 'f'
                    | 't'
                    | 'n'
                    | 'r'
                    | 'v'
                    | 'x'
                    | 'u'
                    | 'U'
                    | 'm'
                    | 'M'
                    | 'y'
                    | 'Y'
                    | 'Z'
            )
        {
            return invalid("unselected constraint escape notation");
        }
        if let Some(bracket) = self.brackets.last_mut() {
            bracket.first = false;
        }
        self.offset += character.len_utf8();
        Ok(())
    }

    fn bracket(&mut self, character: char) -> Result<(), QueryError> {
        match character {
            '[' => {
                if let Some(bracket) = self.brackets.last_mut() {
                    bracket.first = false;
                }
                self.brackets.push(
                    Bracket {
                        first: true,
                        negation: true,
                    },
                    self.controls,
                )?;
            }
            ']' if self.brackets.last().is_some_and(|bracket| !bracket.first) => {
                self.brackets.values.pop();
            }
            '^' if self
                .brackets
                .last()
                .is_some_and(|bracket| bracket.first && bracket.negation) =>
            {
                self.brackets.last_mut().expect("bracket exists").negation = false;
            }
            _ => {
                if let Some(bracket) = self.brackets.last_mut() {
                    bracket.first = false;
                }
            }
        }
        Ok(())
    }

    fn group(&mut self) -> Result<(), QueryError> {
        let suffix = &self.input[self.offset..];
        if suffix.starts_with("(?#") {
            self.offset += 3;
            while self.offset < self.input.len() {
                check_interruption(self.controls)?;
                let character = self.input[self.offset..]
                    .chars()
                    .next()
                    .expect("comment scalar boundary");
                self.offset += character.len_utf8();
                if character == ')' {
                    break;
                }
            }
            return Ok(());
        }
        if suffix.starts_with("(?") {
            let mut end = self.offset + 2;
            let mut expanded = self.expanded;
            let mut enabled = true;
            let mut flags = false;
            while let Some(character) = self.input[end..].chars().next() {
                if character == '-' {
                    enabled = false;
                    end += 1;
                    continue;
                }
                if !character.is_ascii_alphabetic() {
                    break;
                }
                flags = true;
                if !matches!(character, 'i' | 'm' | 's' | 'U' | 'u' | 'x' | 'R') {
                    return invalid("unselected constraint group flag");
                }
                if character == 'x' {
                    expanded = enabled;
                }
                end += 1;
            }
            if flags && self.input[end..].starts_with(')') {
                self.expanded = expanded;
                self.offset = end + 1;
                return Ok(());
            }
            if flags && self.input[end..].starts_with(':') {
                self.scopes.push(self.expanded, self.controls)?;
                self.expanded = expanded;
                self.offset = end + 1;
                return Ok(());
            }
        }
        self.scopes.push(self.expanded, self.controls)?;
        self.offset += 1;
        Ok(())
    }

    fn line_comment(&mut self) -> Result<(), QueryError> {
        while self.offset < self.input.len() {
            check_interruption(self.controls)?;
            let character = self.input[self.offset..]
                .chars()
                .next()
                .expect("comment scalar boundary");
            self.offset += character.len_utf8();
            if character == '\n' {
                break;
            }
        }
        Ok(())
    }
}

fn invalid(message: &str) -> Result<(), QueryError> {
    Err(invalid_regular_expression(message))
}
