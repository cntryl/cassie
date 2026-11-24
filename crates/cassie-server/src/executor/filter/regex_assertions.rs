//! PG constraint syntax uses native assertions after AST-accurate context checks.

use super::pattern_memory::{reserve, syntax_scratch, PatternText};
use super::{check_interruption, invalid_regular_expression, resource_limit, QueryError};
use crate::runtime::QueryExecutionControls;
use regex_syntax::ast::{self, Ast, ClassBracketed, ClassSetItem, Visitor};

struct Replacement {
    start: usize,
    end: usize,
    text: &'static str,
}

struct BracketContexts<'a> {
    replacements: &'a [Replacement],
    controls: Option<&'a QueryExecutionControls>,
}
impl BracketContexts<'_> {
    fn bracket(&self, class: &ClassBracketed) -> Result<(), QueryError> {
        check_interruption(self.controls)?;
        let index = self
            .replacements
            .partition_point(|replacement| replacement.start < class.span.start.offset);
        if self
            .replacements
            .get(index)
            .is_some_and(|replacement| replacement.start < class.span.end.offset)
        {
            return Err(invalid_regular_expression(
                "word or anchor constraint is not allowed in a bracket expression",
            ));
        }
        Ok(())
    }
}
impl Visitor for BracketContexts<'_> {
    type Output = ();
    type Err = QueryError;
    fn finish(self) -> Result<(), QueryError> {
        Ok(())
    }
    fn visit_pre(&mut self, ast: &Ast) -> Result<(), QueryError> {
        if let Ast::ClassBracketed(class) = ast {
            self.bracket(class)?;
        }
        Ok(())
    }
    fn visit_class_set_item_pre(&mut self, item: &ClassSetItem) -> Result<(), QueryError> {
        if let ClassSetItem::Bracketed(class) = item {
            self.bracket(class)?;
        }
        Ok(())
    }
}

pub(super) fn translate(
    pattern: PatternText,
    advanced: bool,
    expanded: bool,
    controls: Option<&QueryExecutionControls>,
) -> Result<PatternText, QueryError> {
    let capacity = pattern
        .as_str()
        .len()
        .checked_mul(2)
        .ok_or_else(|| resource_limit("regex assertion normalization size overflow"))?;
    let record_count = pattern.as_str().len() / 2 + 1;
    let record_bytes = record_count
        .checked_mul(std::mem::size_of::<Replacement>())
        .ok_or_else(|| resource_limit("regex assertion directory size overflow"))?;
    let _records_memory = reserve(controls, record_bytes)?;
    let mut replacements = Vec::new();
    replacements
        .try_reserve_exact(record_count)
        .map_err(|error| {
            resource_limit(&format!(
                "unable to allocate regex assertion directory: {error}"
            ))
        })?;
    if replacements.capacity() > record_count {
        return Err(resource_limit(
            "regex assertion directory exceeded admitted capacity",
        ));
    }
    let normalized = pattern.transform(capacity, controls, |input, writer| {
        normalize_tokens(input, writer, &mut replacements, advanced, controls)
    })?;
    if replacements.is_empty() {
        return Ok(normalized);
    }
    let _syntax_memory = syntax_scratch(normalized.as_str().len(), controls)?;
    let ast = ast::parse::ParserBuilder::new()
        .ignore_whitespace(expanded)
        .build()
        .parse(normalized.as_str())
        .map_err(|error| invalid_regular_expression(&error.to_string()))?;
    ast::visit(
        &ast,
        BracketContexts {
            replacements: &replacements,
            controls,
        },
    )?;
    let length =
        replacements
            .iter()
            .try_fold(normalized.as_str().len(), |length, replacement| {
                length
                    .checked_sub(replacement.end - replacement.start)
                    .and_then(|length| length.checked_add(replacement.text.len()))
                    .ok_or_else(|| resource_limit("regex assertion output size overflow"))
            })?;
    normalized.transform(length, controls, |input, writer| {
        let mut cursor = 0;
        for replacement in &replacements {
            check_interruption(controls)?;
            writer.push_str(&input[cursor..replacement.start])?;
            writer.push_str(replacement.text)?;
            cursor = replacement.end;
        }
        writer.push_str(&input[cursor..])
    })
}

fn normalize_tokens(
    input: &str,
    writer: &mut super::pattern_memory::Writer,
    replacements: &mut Vec<Replacement>,
    advanced: bool,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    let mut offset = 0;
    while offset < input.len() {
        check_interruption(controls)?;
        let suffix = &input[offset..];
        let alias = if suffix.starts_with("[[:<:]]") {
            Some(r"\b{start}")
        } else if suffix.starts_with("[[:>:]]") {
            Some(r"\b{end}")
        } else {
            None
        };
        if let Some(text) = alias {
            let start = writer.len();
            writer.push('a')?;
            replacements.push(Replacement {
                start,
                end: start + 1,
                text,
            });
            offset += 7;
            continue;
        }
        let character = suffix.chars().next().expect("offset within source");
        offset += character.len_utf8();
        if character != '\\' {
            writer.push(character)?;
            continue;
        }
        let Some(escaped) = input[offset..].chars().next() else {
            writer.push('\\')?;
            break;
        };
        offset += escaped.len_utf8();
        if !advanced && matches!(escaped, 'b' | 'B' | 'm' | 'M' | 'y' | 'Y' | 'Z') {
            writer.push(escaped)?;
            continue;
        }
        let constraint = match escaped {
            'm' => Some(r"\b{start}"),
            'M' => Some(r"\b{end}"),
            'y' => Some(r"\b"),
            'Y' => Some(r"\B"),
            'Z' => Some(r"\z"),
            _ => None,
        };
        if advanced {
            if let Some(text) = constraint {
                let start = writer.len();
                writer.push('a')?;
                replacements.push(Replacement {
                    start,
                    end: start + 1,
                    text,
                });
                continue;
            }
            if escaped == 'b' {
                writer.push_str(r"\x08")?;
                continue;
            }
            if escaped == 'B' {
                writer.push_str(r"\\")?;
                continue;
            }
        }
        writer.push('\\')?;
        writer.push(escaped)?;
    }
    Ok(())
}
