//! PostgreSQL newline-sensitive modes exclude LF from every negated bracket class.

use regex_syntax::ast::{self, Ast, ClassBracketed, ClassSetItem, Visitor};

use super::{invalid_regular_expression, resource_limit, QueryError};

#[derive(Default)]
struct NegatedClasses {
    insertions: Vec<(usize, &'static str)>,
}

impl NegatedClasses {
    fn record(&mut self, class: &ClassBracketed) {
        if class.negated {
            // Intersect the entire original class, preserving set-operation precedence,
            // escaped brackets and literal ']' in the first class position.
            self.insertions.push((class.span.start.offset, "["));
            self.insertions.push((class.span.end.offset, r"&&[^\n]]"));
        }
    }
}

impl Visitor for NegatedClasses {
    type Output = Vec<(usize, &'static str)>;
    type Err = QueryError;

    fn finish(self) -> Result<Self::Output, Self::Err> {
        Ok(self.insertions)
    }

    fn visit_pre(&mut self, ast: &Ast) -> Result<(), Self::Err> {
        if let Ast::ClassBracketed(class) = ast {
            self.record(class);
        }
        Ok(())
    }

    fn visit_class_set_item_pre(&mut self, item: &ClassSetItem) -> Result<(), Self::Err> {
        if let ClassSetItem::Bracketed(class) = item {
            self.record(class);
        }
        Ok(())
    }
}

pub(super) fn exclude_newlines(pattern: &str, expanded: bool) -> Result<String, QueryError> {
    let ast = ast::parse::ParserBuilder::new()
        .ignore_whitespace(expanded)
        .build()
        .parse(pattern)
        .map_err(|error| invalid_regular_expression(&error.to_string()))?;
    let mut insertions = ast::visit(&ast, NegatedClasses::default())?;
    // Stable ordering puts the suffix of a class before the prefix of an
    // immediately adjacent class, including nested source spans.
    insertions.sort_by_key(|(offset, _)| *offset);
    let length = insertions
        .iter()
        .try_fold(pattern.len(), |length, (_, text)| {
            length
                .checked_add(text.len())
                .ok_or_else(|| resource_limit("regular-expression pattern size overflow"))
        })?;
    let mut output = String::new();
    output.try_reserve_exact(length).map_err(|error| {
        resource_limit(&format!(
            "unable to allocate regular-expression pattern: {error}"
        ))
    })?;
    let mut previous = 0;
    for (offset, text) in insertions {
        output.push_str(&pattern[previous..offset]);
        output.push_str(text);
        previous = offset;
    }
    output.push_str(&pattern[previous..]);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::exclude_newlines;
    use regex_automata::meta::Regex;

    #[test]
    fn should_preserve_nested_class_precedence_when_excluding_newlines() {
        // Arrange
        let patterns = [r"[^a&&[^b]]", r"[^a][^b]", r"[^]x]"];

        // Act
        let rewritten =
            patterns.map(|pattern| exclude_newlines(pattern, false).expect("class AST"));
        let regexes = rewritten.map(|pattern| Regex::new(&pattern).expect("rewritten regex"));

        // Assert
        assert!(!regexes[0].is_match("\n"));
        assert!(!regexes[0].is_match("a"));
        assert!(regexes[0].is_match("b"));
        assert!(regexes[0].is_match("猫"));
        assert!(!regexes[1].is_match("x\n"));
        assert!(regexes[1].is_match("猫犬"));
        assert!(!regexes[2].is_match("]"));
        assert!(!regexes[2].is_match("\n"));
        assert!(regexes[2].is_match("猫"));
    }

    #[test]
    fn should_leave_non_class_pattern_text_unchanged() {
        // Arrange
        let escaped = r"\[\^x\]";
        let expanded = "a # [^x] is a comment\n b";

        // Act
        let literal = exclude_newlines(escaped, false).expect("escaped bracket tokens");
        let comment = exclude_newlines(expanded, true).expect("expanded comment");

        // Assert
        assert_eq!(literal, escaped);
        assert_eq!(comment, expanded);
    }

    #[test]
    fn should_preserve_non_newline_class_matches_after_newline_exclusion() {
        // Arrange
        let patterns = [
            r"[^a-z]",
            r"[^]a]",
            r"[^猫]",
            r"[^\\]",
            r"[^a][^b]",
            r"[^a&&[^b]]",
        ];
        let values = [
            "", "a", "b", "]", "猫", "犬", "ab", "猫犬", "\\", "é", "\r", "\u{2028}",
        ];

        // Act
        let observations = patterns.map(|pattern| {
            let original = Regex::new(pattern).expect("original backend class");
            let rewritten = exclude_newlines(pattern, false).expect("class rewrite");
            let corrected = Regex::new(&rewritten).expect("corrected backend class");
            (
                values.map(|value| (original.is_match(value), corrected.is_match(value))),
                corrected.is_match("\n"),
            )
        });

        // Assert
        for (pattern, (matches, newline)) in patterns.into_iter().zip(observations) {
            assert!(!newline, "negated class must exclude LF: {pattern}");
            for (value, (original, corrected)) in values.into_iter().zip(matches) {
                assert_eq!(original, corrected, "pattern {pattern}, value {value:?}");
            }
        }
    }
}
