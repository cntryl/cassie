use std::fmt::{Display, Formatter};
use std::ops::Deref;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One SQL identifier component, with its original delimiter state retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentifierComponent {
    value: String,
    delimited: bool,
}

impl IdentifierComponent {
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }

    #[must_use]
    pub const fn is_delimited(&self) -> bool {
        self.delimited
    }

    #[must_use]
    pub fn delimited(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            delimited: true,
        }
    }

    #[must_use]
    pub fn undelimited(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            delimited: false,
        }
    }
}

/// A one-to-three-part SQL name that keeps quoted component boundaries.
///
/// The rendered string is a reversible, canonical SQL identifier path. It is
/// retained for existing planner and catalog interfaces; parser and binder
/// code should use `components()` when interpreting a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierPath {
    components: Vec<IdentifierComponent>,
    rendered: String,
}

#[derive(Serialize)]
struct IdentifierPathRef<'a> {
    components: &'a [IdentifierComponent],
}

#[derive(Deserialize)]
struct IdentifierPathOwned {
    components: Vec<IdentifierComponent>,
}

impl IdentifierPath {
    /// Parses a dotted SQL identifier path, retaining each component and
    /// whether it was delimited.
    ///
    /// # Errors
    ///
    /// Returns an error for empty, malformed, or more-than-three-part paths.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let mut remaining = raw.trim();
        if remaining.is_empty() {
            return Err("empty identifier path".to_string());
        }

        let mut components = Vec::new();
        loop {
            remaining = remaining.trim_start();
            if remaining.is_empty() {
                return Err(format!("invalid qualified name '{raw}'"));
            }

            let (component, after) = if let Some(body) = remaining.strip_prefix('"') {
                let (value, consumed) = take_quoted_component(body)
                    .ok_or_else(|| format!("unterminated quoted identifier '{raw}'"))?;
                if value.is_empty() {
                    return Err("empty quoted identifier".to_string());
                }
                let after = body[consumed..].trim_start();
                (
                    IdentifierComponent {
                        value,
                        delimited: true,
                    },
                    after,
                )
            } else {
                let end = remaining.find('.').unwrap_or(remaining.len());
                let value = remaining[..end].trim();
                if value.is_empty() || value.contains('"') {
                    return Err(format!("invalid qualified name '{raw}'"));
                }
                (
                    IdentifierComponent {
                        value: value.to_string(),
                        delimited: false,
                    },
                    &remaining[end..],
                )
            };
            components.push(component);
            if components.len() > 3 {
                return Err(format!("unsupported qualified name '{raw}'"));
            }

            if after.is_empty() {
                break;
            }
            let Some(next) = after.strip_prefix('.') else {
                return Err(format!("invalid qualified name '{raw}'"));
            };
            remaining = next;
        }

        Ok(Self::from_components(components))
    }

    #[must_use]
    pub fn from_values(values: impl IntoIterator<Item = String>) -> Self {
        Self::from_components(
            values
                .into_iter()
                .map(|value| IdentifierComponent {
                    value,
                    delimited: false,
                })
                .collect(),
        )
    }

    /// Builds an exact single-component path for a name already resolved from
    /// catalog metadata.
    #[must_use]
    pub fn from_field_name(value: impl Into<String>) -> Self {
        Self::from_components(vec![IdentifierComponent::delimited(value)])
    }

    #[must_use]
    pub fn from_components(components: Vec<IdentifierComponent>) -> Self {
        let rendered = components
            .iter()
            .map(|component| crate::catalog::canonical_identifier_component(&component.value))
            .collect::<Vec<_>>()
            .join(".");
        Self {
            components,
            rendered,
        }
    }

    #[must_use]
    pub fn components(&self) -> &[IdentifierComponent] {
        &self.components
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    pub fn values(&self) -> impl Iterator<Item = &str> {
        self.components.iter().map(IdentifierComponent::value)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.rendered
    }

    #[must_use]
    pub fn into_string(self) -> String {
        self.rendered
    }

    #[must_use]
    pub fn local_component(&self) -> Option<&IdentifierComponent> {
        self.components.last()
    }

    /// Returns the resolved spelling of the final column component. An
    /// undelimited SQL identifier folds to lowercase; a delimited component
    /// retains its exact catalog spelling.
    #[must_use]
    pub fn column_name(&self) -> Option<String> {
        self.local_component().map(|component| {
            if component.is_delimited() {
                component.value().to_string()
            } else {
                component.value().to_ascii_lowercase()
            }
        })
    }

    /// Returns an unambiguous lookup key for a column reference. Relation
    /// qualifiers retain Cassie's existing case-insensitive behavior; only
    /// the final column component distinguishes delimited case.
    #[must_use]
    pub fn column_lookup_key(&self) -> String {
        let last = self.components.len().saturating_sub(1);
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

    /// Converts the retained components to the catalog's one-, two-, or
    /// three-part name variants without reparsing rendered text.
    ///
    /// # Errors
    ///
    /// Returns an error when the path has an unsupported number of parts.
    pub fn parsed_name(&self) -> Result<crate::catalog::ParsedName, String> {
        match self.components.as_slice() {
            [name] => Ok(crate::catalog::ParsedName::Unqualified(name.value.clone())),
            [schema, name] => Ok(crate::catalog::ParsedName::SchemaQualified {
                schema: schema.value.clone(),
                name: name.value.clone(),
            }),
            [database, schema, name] => Ok(crate::catalog::ParsedName::DatabaseQualified {
                database: database.value.clone(),
                schema: schema.value.clone(),
                name: name.value.clone(),
            }),
            _ => Err(format!("unsupported qualified name '{self}'")),
        }
    }
}

impl Deref for IdentifierPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for IdentifierPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq<&str> for IdentifierPath {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for IdentifierPath {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl Display for IdentifierPath {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for IdentifierPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        IdentifierPathRef {
            components: &self.components,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for IdentifierPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = IdentifierPathOwned::deserialize(deserializer)?;
        Ok(Self::from_components(path.components))
    }
}

fn take_quoted_component(body: &str) -> Option<(String, usize)> {
    let mut value = String::new();
    let mut chars = body.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        if character != '"' {
            value.push(character);
            continue;
        }
        if matches!(chars.peek(), Some((_, '"'))) {
            value.push('"');
            chars.next();
            continue;
        }
        return Some((value, index + 1));
    }
    None
}
