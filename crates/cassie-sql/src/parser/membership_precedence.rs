//! IN and BETWEEN bind tighter than the comparison operators.
//!
//! `a = b BETWEEN c AND d` is `a = (b BETWEEN c AND d)`. The expression parser splits on the
//! lowest-precedence operator first, so when the text before a top-level IN or BETWEEN keyword
//! holds a top-level comparison, that comparison is the split point and the membership test
//! belongs to its right operand.

const DISTANCE_OPERATORS: [&str; 3] = [" <=> ", " <-> ", " <#> "];

/// Whether `left`, the text before a top-level IN or BETWEEN keyword, has a top-level
/// comparison operator that must be split first.
pub(super) fn left_operand_has_comparison(left: &str) -> bool {
    // The distance operators bind tighter than comparisons and have their own handling.
    if DISTANCE_OPERATORS.iter().any(|token| left.contains(token)) {
        return false;
    }
    super::operators::has_ordinary_comparison(left)
}

#[cfg(test)]
mod tests {
    use super::left_operand_has_comparison;

    #[test]
    fn should_find_a_top_level_comparison_before_the_keyword() {
        // Arrange
        let left = "false = true";

        // Act
        let found = left_operand_has_comparison(left);

        // Assert
        assert!(found);
    }

    #[test]
    fn should_ignore_a_parenthesized_comparison_before_the_keyword() {
        // Arrange
        let left = "(a = b)";

        // Act
        let found = left_operand_has_comparison(left);

        // Assert
        assert!(!found);
    }

    #[test]
    fn should_ignore_a_comparison_inside_a_string_literal() {
        // Arrange
        let left = "'a = b'";

        // Act
        let found = left_operand_has_comparison(left);

        // Assert
        assert!(!found);
    }

    #[test]
    fn should_leave_a_distance_operand_to_its_own_handling() {
        // Arrange
        let left = "embedding <-> '[1,2]'";

        // Act
        let found = left_operand_has_comparison(left);

        // Assert
        assert!(!found);
    }
}
