use crate::sql::ast::IdentifierComponent;

/// A column reference keeps delimiter state for its final component while
/// retaining the engine's existing case-insensitive relation qualifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColumnIdentifierPath {
    components: Vec<IdentifierComponent>,
}

impl ColumnIdentifierPath {
    /// Retains quoted qualifier identity for an alias namespace. Catalog
    /// qualifier lookup keeps using its historical folded key.
    pub(crate) fn namespace_key(&self) -> String {
        let last = self.components.len() - 1;
        self.components
            .iter()
            .enumerate()
            .map(|(index, component)| {
                if index == last {
                    Self {
                        components: vec![component.clone()],
                    }
                    .lookup_key()
                } else if component.is_delimited()
                    && (component.value() != component.value().to_ascii_lowercase()
                        || component.value().contains(['.', '"'])
                        || component.value().chars().any(char::is_whitespace))
                {
                    format!("\"{}\"", component.value().replace('"', "\"\""))
                } else {
                    component.value().to_ascii_lowercase()
                }
            })
            .collect::<Vec<_>>()
            .join(".")
    }

    pub(crate) fn namespace_qualifier(&self) -> Option<String> {
        (self.components.len() == 2).then(|| {
            let component = &self.components[0];
            if component.is_delimited() {
                component.value().to_string()
            } else {
                component.value().to_ascii_lowercase()
            }
        })
    }
    /// Returns the canonical key for a SQL column reference's final component.
    pub(crate) fn reference_field_key(raw: &str) -> String {
        Self::parse(raw).map_or_else(|_| raw.to_ascii_lowercase(), |path| path.field_lookup_key())
    }

    /// Returns the canonical key for a catalog field's stored spelling.
    pub(crate) fn stored_field_key(raw: &str) -> String {
        Self::from_field_name(raw).lookup_key()
    }

    /// Returns a row entry key with relation qualifiers folded and the final
    /// stored field spelling retained exactly.
    pub(crate) fn stored_row_lookup_key(raw: &str) -> String {
        let mut path = Self::parse(raw).unwrap_or_else(|_| Self::from_field_name(raw));
        let final_index = path.components.len() - 1;
        let final_name = path.components[final_index].value().to_string();
        path.components[final_index] = IdentifierComponent::delimited(final_name);
        path.lookup_key()
    }

    /// Returns the exact stored spelling of a row entry's final field.
    pub(crate) fn stored_row_field_key(raw: &str) -> String {
        let path = Self::parse(raw).unwrap_or_else(|_| Self::from_field_name(raw));
        Self::stored_field_key(path.final_name())
    }

    /// Matches either a literal catalog field name or an encoded SQL column
    /// reference to a stored catalog field name.
    pub(crate) fn matches_stored_field(reference: &str, stored_name: &str) -> bool {
        reference == stored_name
            || Self::reference_field_key(reference) == Self::stored_field_key(stored_name)
    }

    /// Parses a column reference, including a three-part relation qualifier.
    pub(crate) fn parse(raw: &str) -> Result<Self, String> {
        let mut remaining = raw.trim();
        if remaining.is_empty() {
            return Err("empty column reference".to_string());
        }

        let mut components = Vec::new();
        loop {
            remaining = remaining.trim_start();
            if remaining.is_empty() {
                return Err(format!("invalid column reference '{raw}'"));
            }

            let (component, after) = if let Some(body) = remaining.strip_prefix('"') {
                let (value, consumed) = take_delimited_body(body)
                    .ok_or_else(|| format!("unterminated quoted identifier '{raw}'"))?;
                if value.is_empty() {
                    return Err("empty quoted identifier".to_string());
                }
                let after = body[consumed..].trim_start();
                (IdentifierComponent::delimited(value), after)
            } else {
                let end = remaining.find('.').unwrap_or(remaining.len());
                let value = remaining[..end].trim();
                if value.is_empty() || value.contains('"') {
                    return Err(format!("invalid column reference '{raw}'"));
                }
                (IdentifierComponent::undelimited(value), &remaining[end..])
            };
            components.push(component);
            if components.len() > 4 {
                return Err(format!("unsupported column reference '{raw}'"));
            }

            if after.is_empty() {
                break;
            }
            let Some(next) = after.strip_prefix('.') else {
                return Err(format!("invalid column reference '{raw}'"));
            };
            remaining = next;
        }

        Ok(Self { components })
    }

    pub(crate) fn from_field_name(name: &str) -> Self {
        Self {
            components: vec![IdentifierComponent::delimited(name)],
        }
    }

    pub(crate) fn matches_field_name(&self, name: &str) -> bool {
        self.field_lookup_key() == Self::from_field_name(name).lookup_key()
    }

    /// Returns the canonical key used by binder and executor lookup.
    pub(crate) fn lookup_key(&self) -> String {
        let last = self.components.len() - 1;
        self.components
            .iter()
            .enumerate()
            .map(|(index, component)| {
                let preserve_case = index == last && component.is_delimited();
                let value = if preserve_case {
                    component.value().to_string()
                } else {
                    component.value().to_ascii_lowercase()
                };
                if preserve_case
                    && (value != value.to_ascii_lowercase()
                        || value.contains(['.', '"'])
                        || value.chars().any(char::is_whitespace))
                {
                    format!("\"{}\"", value.replace('"', "\"\""))
                } else {
                    crate::catalog::canonical_identifier_component(&value)
                }
            })
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Returns the key for the final component, with its SQL folding applied.
    pub(crate) fn field_lookup_key(&self) -> String {
        let component = self
            .components
            .last()
            .expect("column identifier paths contain a component");
        Self {
            components: vec![component.clone()],
        }
        .lookup_key()
    }

    pub(crate) fn final_name(&self) -> &str {
        self.components
            .last()
            .expect("column identifier paths contain a component")
            .value()
    }

    pub(crate) fn declared_name(&self) -> String {
        let component = self
            .components
            .last()
            .expect("column identifier paths contain a component");
        if component.is_delimited() {
            component.value().to_string()
        } else {
            component.value().to_ascii_lowercase()
        }
    }

    pub(crate) fn is_qualified(&self) -> bool {
        self.components.len() > 1
    }

    /// Candidate row keys in qualifier preference order, then the bare field.
    pub(crate) fn row_lookup_candidates(&self) -> Vec<String> {
        let last = self.components.len() - 1;
        let field = self.final_name();
        let mut candidates = Vec::with_capacity(last + 1);
        for start in 0..last {
            let qualifier = self.components[start..last]
                .iter()
                .map(|component| {
                    crate::catalog::canonical_identifier_component(
                        &component.value().to_ascii_lowercase(),
                    )
                })
                .collect::<Vec<_>>()
                .join(".");
            candidates.push(format!("{qualifier}.{field}"));
        }
        candidates.push(field.to_string());
        candidates
    }

    /// Keeps the historical projection label spelling while removing SQL
    /// delimiter syntax used only by the internal lookup key.
    pub(crate) fn display_name(&self) -> String {
        self.components
            .iter()
            .map(IdentifierComponent::value)
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Returns parseable SQL that keeps the exact final column spelling.
    pub(crate) fn canonical_sql(&self) -> String {
        let last = self.components.len() - 1;
        self.components
            .iter()
            .enumerate()
            .map(|(index, component)| {
                if index == last {
                    format!("\"{}\"", component.value().replace('"', "\"\""))
                } else {
                    crate::catalog::canonical_identifier_component(
                        &component.value().to_ascii_lowercase(),
                    )
                }
            })
            .collect::<Vec<_>>()
            .join(".")
    }
}

fn take_delimited_body(body: &str) -> Option<(String, usize)> {
    let mut value = String::new();
    let mut chars = body.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch != '"' {
            value.push(ch);
            continue;
        }
        if matches!(chars.peek(), Some((_, '"'))) {
            value.push('"');
            chars.next();
            continue;
        }
        return Some((value, idx + 1));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::ColumnIdentifierPath;

    #[test]
    fn should_fold_only_undelimited_column_components() {
        // Arrange
        let unquoted =
            ColumnIdentifierPath::parse("DB.Schema.Users.Email").expect("column path parses");
        let quoted = ColumnIdentifierPath::parse("DB.Schema.Users.\"Email\"")
            .expect("quoted column path parses");

        // Act
        let unquoted_key = unquoted.lookup_key();
        let quoted_key = quoted.lookup_key();

        // Assert
        assert_eq!(unquoted_key, "db.schema.users.email");
        assert_eq!(quoted_key, "db.schema.users.\"Email\"");
    }

    #[test]
    fn should_encode_a_delimited_column_that_contains_a_dot() {
        // Arrange
        let column = ColumnIdentifierPath::parse("\"a.b\"").expect("column path parses");

        // Act
        let key = column.lookup_key();
        let decoded = ColumnIdentifierPath::parse(&key).expect("encoded key parses");

        // Assert
        assert_eq!(key, "\"a.b\"");
        assert_eq!(decoded.final_name(), "a.b");
        assert!(!decoded.is_qualified());
    }

    #[test]
    fn should_format_canonical_sql_without_reescaping_delimited_column_names() {
        // Arrange
        let column = ColumnIdentifierPath::parse("\"odd\"\"name\"").expect("column path parses");

        // Act
        let sql = column.canonical_sql();

        // Assert
        assert_eq!(sql, "\"odd\"\"name\"");
        assert_eq!(
            ColumnIdentifierPath::parse(&sql)
                .expect("canonical SQL parses")
                .final_name(),
            "odd\"name"
        );
    }
}
