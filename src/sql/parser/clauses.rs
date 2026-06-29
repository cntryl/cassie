use super::SqlError;

#[derive(Debug, Clone, Copy)]
pub(super) enum Clause {
    Where,
    Group,
    Having,
    Order,
    Limit,
    Offset,
}

impl Clause {
    pub(super) fn token(self) -> &'static str {
        match self {
            Self::Where => "where",
            Self::Group => "group by",
            Self::Having => "having",
            Self::Order => "order by",
            Self::Limit => "limit",
            Self::Offset => "offset",
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Where => "WHERE",
            Self::Group => "GROUP BY",
            Self::Having => "HAVING",
            Self::Order => "ORDER BY",
            Self::Limit => "LIMIT",
            Self::Offset => "OFFSET",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ClauseToken {
    Recognized(Clause),
    Unsupported(&'static str),
}

#[derive(Debug)]
pub(super) struct ClauseMatch {
    pub(super) position: usize,
    pub(super) token: ClauseToken,
}

impl ClauseMatch {
    pub(super) fn text(&self) -> &'static str {
        match self.token {
            ClauseToken::Recognized(kind) => kind.name(),
            ClauseToken::Unsupported(text) => text,
        }
    }
}

pub(super) fn parse_clauses(rest: &str) -> Result<Vec<ClauseMatch>, SqlError> {
    let mut matches = Vec::new();

    for token in [
        ("where", ClauseToken::Recognized(Clause::Where)),
        ("group by", ClauseToken::Recognized(Clause::Group)),
        ("having", ClauseToken::Recognized(Clause::Having)),
        ("order by", ClauseToken::Recognized(Clause::Order)),
        ("limit", ClauseToken::Recognized(Clause::Limit)),
        ("offset", ClauseToken::Recognized(Clause::Offset)),
        ("intersect", ClauseToken::Unsupported("INTERSECT")),
        ("except", ClauseToken::Unsupported("EXCEPT")),
    ] {
        let mut cursor = 0;
        while let Some(position) = find_top_level_clause(rest, cursor, token.0) {
            matches.push(ClauseMatch {
                position,
                token: token.1,
            });
            cursor = position + 1;
        }
    }

    matches.sort_by_key(|entry| entry.position);

    for window in matches.windows(2) {
        if window[0].position == window[1].position {
            return Err(SqlError(format!(
                "ambiguous clause token '{}' at position {}",
                window[0].text(),
                window[0].position,
            )));
        }
    }

    let mut ordered = Vec::new();
    for clause in matches {
        if let ClauseToken::Unsupported(kind) = clause.token {
            return Err(SqlError(format!("unsupported clause '{kind}'")));
        }
        ordered.push(clause);
    }

    Ok(ordered)
}

pub(super) fn find_top_level_keyword(rest: &str, start: usize, token: &str) -> Option<usize> {
    find_top_level_clause(rest, start, token)
}

pub(super) fn find_top_level_clause(rest: &str, start: usize, token: &str) -> Option<usize> {
    let lower = rest.to_lowercase();
    let token = token.as_bytes();
    let bytes = lower.as_bytes();
    let mut depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;

    for (idx, ch) in lower.char_indices() {
        if idx < start {
            match ch {
                '\'' if !in_double => in_single = !in_single,
                '"' if !in_single => in_double = !in_double,
                '(' if !in_single && !in_double => depth += 1,
                ')' if !in_single && !in_double => depth = depth.saturating_sub(1),
                '[' if !in_single && !in_double => bracket_depth += 1,
                ']' if !in_single && !in_double => bracket_depth = bracket_depth.saturating_sub(1),
                _ => {}
            }
            continue;
        }

        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => depth = depth.saturating_sub(1),
            '[' if !in_single && !in_double => bracket_depth += 1,
            ']' if !in_single && !in_double => bracket_depth = bracket_depth.saturating_sub(1),
            _ => {}
        }

        if depth != 0 || bracket_depth != 0 || in_single || in_double {
            continue;
        }

        if idx + token.len() > bytes.len() {
            continue;
        }

        if &bytes[idx..idx + token.len()] == token
            && is_clause_boundary_before(lower.as_bytes(), idx)
            && is_clause_boundary_after(lower.as_bytes(), idx + token.len())
        {
            return Some(idx);
        }
    }

    None
}

pub(super) fn is_clause_boundary_before(bytes: &[u8], index: usize) -> bool {
    index == 0 || !is_identifier_byte(*bytes.get(index.saturating_sub(1)).unwrap_or(&b' '))
}

pub(super) fn is_clause_boundary_after(bytes: &[u8], index: usize) -> bool {
    index >= bytes.len() || !is_identifier_byte(*bytes.get(index).unwrap_or(&b' '))
}

pub(super) fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

pub(super) fn split_top_level<'a>(input: &'a str, keyword: &'a str) -> Option<(&'a str, &'a str)> {
    let lower = input.to_lowercase();
    let chars = lower.char_indices().collect::<Vec<_>>();
    let token = keyword.as_bytes();
    let mut depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;

    for &(idx, ch) in &chars {
        match ch {
            '\'' => {
                if !in_double {
                    in_single = !in_single;
                }
            }
            '"' => {
                if !in_single {
                    in_double = !in_double;
                }
            }
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => depth = depth.saturating_sub(1),
            '[' if !in_single && !in_double => bracket_depth += 1,
            ']' if !in_single && !in_double => bracket_depth = bracket_depth.saturating_sub(1),
            _ => {}
        }

        if depth == 0
            && bracket_depth == 0
            && !in_single
            && !in_double
            && idx + token.len() <= input.len()
        {
            let slice = &lower[idx..idx + token.len()];
            if slice.as_bytes() == token {
                return Some((&input[..idx], &input[idx + token.len()..]));
            }
        }
    }

    None
}

pub(super) fn strip_parentheses(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
        return None;
    }

    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    for (i, ch) in trimmed.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => depth -= 1,
            _ => {}
        }

        if depth == 0 && i != trimmed.len().saturating_sub(1) {
            return None;
        }
    }

    Some(trimmed[1..trimmed.len() - 1].trim())
}
