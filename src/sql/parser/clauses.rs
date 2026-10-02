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
    pub(super) end: usize,
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
        while let Some((position, end)) = find_top_level_keyword_span(rest, cursor, token.0) {
            matches.push(ClauseMatch {
                position,
                end,
                token: token.1,
            });
            cursor = position + 1;
        }
    }

    matches.sort_by_key(|entry| entry.position);

    for window in matches.windows(2) {
        if window[0].position == window[1].position {
            return Err(SqlError::new(format!(
                "ambiguous clause token '{}' at position {}",
                window[0].text(),
                window[0].position,
            )));
        }
    }

    let mut ordered = Vec::new();
    for clause in matches {
        if let ClauseToken::Unsupported(kind) = clause.token {
            return Err(SqlError::new(format!("unsupported clause '{kind}'")));
        }
        ordered.push(clause);
    }

    Ok(ordered)
}

pub(super) fn find_top_level_keyword(rest: &str, start: usize, token: &str) -> Option<usize> {
    find_top_level_clause(rest, start, token)
}

/// Finds the first top-level `UNION` and returns its byte offset, the byte
/// length of the whole operator, and whether it is `UNION ALL`. `ALL` may be
/// separated from `UNION` by any whitespace, including newlines.
pub(super) fn find_top_level_union(rest: &str) -> Option<(usize, usize, bool)> {
    let (position, end) = find_top_level_keyword_span(rest, 0, "union")?;
    let next = super::lexical::separator_end(rest, end);
    if next > end {
        if let Some(all_end) = super::lexical::pattern_end(rest, next, "all") {
            return Some((position, all_end - position, true));
        }
    }
    Some((position, end - position, false))
}

pub(super) fn find_top_level_clause(rest: &str, start: usize, token: &str) -> Option<usize> {
    find_top_level_keyword_span(rest, start, token).map(|(position, _)| position)
}

pub(super) fn find_top_level_keyword_span(
    rest: &str,
    start: usize,
    token: &str,
) -> Option<(usize, usize)> {
    super::lexical::find_top_level_span(rest, start, token)
}

pub(super) fn split_top_level<'a>(input: &'a str, keyword: &str) -> Option<(&'a str, &'a str)> {
    let (start, end) = find_top_level_keyword_span(input, 0, keyword)?;
    Some((&input[..start], &input[end..]))
}

pub(super) fn split_top_level_last<'a>(
    input: &'a str,
    keyword: &str,
) -> Option<(&'a str, &'a str)> {
    let mut cursor = 0;
    let mut selected = None;
    while let Some((start, end)) = find_top_level_keyword_span(input, cursor, keyword) {
        selected = Some((&input[..start], &input[end..]));
        cursor = end;
    }
    selected
}

pub(super) fn strip_parentheses(raw: &str) -> Option<&str> {
    let trimmed = super::lexical::trim_separators(raw);
    let close = super::lexical::matching_paren(trimmed, 0)?;
    if close + 1 != trimmed.len() {
        return None;
    }
    Some(super::lexical::trim_separators(&trimmed[1..close]))
}
