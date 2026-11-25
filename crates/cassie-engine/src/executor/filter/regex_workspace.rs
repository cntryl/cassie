//! Source-qualified pre-admission of native AST/HIR and forward compiler scratch.

use super::pattern_memory::syntax_scratch;
use super::{check_interruption, invalid_regular_expression, resource_limit, QueryError};
use crate::runtime::QueryExecutionControls;
use regex_automata::util::syntax;
use regex_syntax::ast::{self, Ast, ClassSetItem, Flag, Flags, FlagsItemKind, GroupKind, Visitor};

const SYMBOLIC_CLASS_RANGES: usize = 8192;
const UTF8_COMPILER_BYTES: usize = 1024 * 1024;

#[derive(Default)]
struct Shape {
    properties: usize,
    dots: usize,
    literals: usize,
    class_literals: usize,
    non_ascii_literals: usize,
    non_ascii_class_literals: usize,
    ascii_fold_special: usize,
    ascii_ranges: usize,
    unicode_ranges: usize,
    ascii_classes: usize,
    negations: usize,
    folded: bool,
}

impl Shape {
    fn flags(&mut self, flags: &Flags) {
        self.folded |= flags
            .items
            .iter()
            .any(|item| matches!(item.kind, FlagsItemKind::Flag(Flag::CaseInsensitive)));
    }

    fn literal(&mut self, character: char, in_class: bool) {
        if in_class {
            self.class_literals += 1;
            self.non_ascii_class_literals += usize::from(!character.is_ascii());
        } else {
            self.literals += 1;
            self.non_ascii_literals += usize::from(!character.is_ascii());
        }
        // These are the only ASCII simple-fold classes with non-ASCII members
        // in the locked Unicode16 table: k/K/Kelvin and s/S/long-s.
        self.ascii_fold_special += usize::from(matches!(character, 'k' | 'K' | 's' | 'S'));
    }

    fn bytes(&self, pattern_bytes: usize) -> Result<usize, QueryError> {
        let mut ranges = mul(self.properties, SYMBOLIC_CLASS_RANGES)?;
        ranges = add(ranges, mul(self.dots, 4)?)?;
        ranges = add(ranges, mul(self.literals, if self.folded { 4 } else { 0 })?)?;
        ranges = add(
            ranges,
            mul(self.class_literals, if self.folded { 4 } else { 1 })?,
        )?;
        ranges = add(
            ranges,
            mul(
                self.ascii_ranges,
                if self.folded {
                    SYMBOLIC_CLASS_RANGES
                } else {
                    1
                },
            )?,
        )?;
        ranges = add(ranges, mul(self.unicode_ranges, SYMBOLIC_CLASS_RANGES)?)?;
        ranges = add(
            ranges,
            mul(self.ascii_classes, if self.folded { 512 } else { 128 })?,
        )?;
        ranges = add(ranges, self.negations)?;
        let unicode = self.properties > 0
            || self.dots > 0
            || self.unicode_ranges > 0
            || self.non_ascii_class_literals > 0
            || self.negations > 0
            || self.folded
                && (self.non_ascii_literals > 0
                    || self.ascii_fold_special > 0
                    || self.ascii_ranges > 0
                    || self.ascii_classes > 0);
        let metadata = add(mul(pattern_bytes, 1024)?, 4096)?;
        let ranges = mul(
            mul(
                ranges,
                std::mem::size_of::<regex_syntax::hir::ClassUnicodeRange>(),
            )?,
            8,
        )?;
        add(
            add(metadata, ranges)?,
            if unicode { UTF8_COMPILER_BYTES } else { 0 },
        )
    }
}

struct Survey<'a> {
    shape: Shape,
    controls: Option<&'a QueryExecutionControls>,
}
impl Visitor for Survey<'_> {
    type Output = Shape;
    type Err = QueryError;
    fn finish(self) -> Result<Shape, QueryError> {
        Ok(self.shape)
    }
    fn visit_pre(&mut self, ast: &Ast) -> Result<(), QueryError> {
        check_interruption(self.controls)?;
        match ast {
            Ast::Literal(literal) => self.shape.literal(literal.c, false),
            Ast::Dot(_) => self.shape.dots += 1,
            Ast::ClassUnicode(_) | Ast::ClassPerl(_) => self.shape.properties += 1,
            Ast::ClassBracketed(class) => self.shape.negations += usize::from(class.negated),
            Ast::Flags(flags) => self.shape.flags(&flags.flags),
            Ast::Group(group) => {
                if let GroupKind::NonCapturing(flags) = &group.kind {
                    self.shape.flags(flags);
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn visit_class_set_item_pre(&mut self, item: &ClassSetItem) -> Result<(), QueryError> {
        check_interruption(self.controls)?;
        match item {
            ClassSetItem::Literal(literal) => self.shape.literal(literal.c, true),
            ClassSetItem::Range(range) => {
                if range.start.c.is_ascii() && range.end.c.is_ascii() {
                    self.shape.ascii_ranges += 1;
                } else {
                    self.shape.unicode_ranges += 1;
                }
            }
            ClassSetItem::Ascii(class) => {
                self.shape.ascii_classes += 1;
                self.shape.negations += usize::from(class.negated);
            }
            ClassSetItem::Unicode(_) | ClassSetItem::Perl(_) => self.shape.properties += 1,
            ClassSetItem::Bracketed(class) => self.shape.negations += usize::from(class.negated),
            _ => {}
        }
        Ok(())
    }
}

pub(super) fn bytes(
    pattern: &str,
    syntax: syntax::Config,
    controls: Option<&QueryExecutionControls>,
) -> Result<usize, QueryError> {
    check_interruption(controls)?;
    if !syntax.get_case_insensitive()
        && !syntax.get_ignore_whitespace()
        && !pattern.chars().any(regex_syntax::is_meta_character)
    {
        // Tight, case-sensitive literals never construct a Unicode class or
        // native UTF8 cache. Their parser/HIR work fits the NFA envelope.
        return Ok(0);
    }
    let _analysis_memory = syntax_scratch(pattern.len(), controls)?;
    let ast = ast::parse::ParserBuilder::new()
        .nest_limit(syntax.get_nest_limit())
        .ignore_whitespace(syntax.get_ignore_whitespace())
        .build()
        .parse(pattern)
        .map_err(|error| invalid_regular_expression(&error.to_string()))?;
    let shape = ast::visit(
        &ast,
        Survey {
            shape: Shape {
                folded: syntax.get_case_insensitive(),
                ..Shape::default()
            },
            controls,
        },
    )?;
    shape.bytes(pattern.len())
}

fn add(left: usize, right: usize) -> Result<usize, QueryError> {
    left.checked_add(right)
        .ok_or_else(|| resource_limit("regex compiler workspace size overflow"))
}
fn mul(left: usize, right: usize) -> Result<usize, QueryError> {
    left.checked_mul(right)
        .ok_or_else(|| resource_limit("regex compiler workspace size overflow"))
}

#[cfg(test)]
mod tests {
    use super::bytes;
    use regex_automata::util::syntax;

    #[test]
    fn should_account_for_hostile_unicode_workspace_before_compilation() {
        // Arrange
        let cases = [
            r"\w",
            r"\p{Age:16.0}",
            r"(?i:[\p{Grapheme_Base}--\p{ASCII}])",
            r"[^\p{Letter}]",
            r"[\p{Greek}~~\p{Cyrillic}]",
            r"(?i:k)",
            r"(?i:[a-z])",
            r"(?-u:\w)",
        ];

        // Act
        let bounds = cases.map(|pattern| {
            bytes(pattern, syntax::Config::new(), None).expect("syntax-only workspace survey")
        });

        // Assert
        for (pattern, bound) in cases.into_iter().zip(bounds) {
            assert!(
                bound > 1024 * 1024,
                "Unicode compiler cache included for {pattern}"
            );
        }
        assert_eq!(
            bytes("plain", syntax::Config::new(), None).expect("literal fast path"),
            0
        );
    }
}
