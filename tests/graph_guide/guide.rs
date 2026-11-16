//! Reads the SQL examples out of `docs/graph-guide.md`, so the guide and its tests cannot
//! drift: the document is the only copy of every statement and every documented result.
//!
//! An example is a ` ```sql ` block preceded by `<!-- example: ID key=value ... -->`. A
//! following ` ```text result ` block holds the expected rows, one line per row with values
//! joined by ` | `; a following ` ```text sqlstate ` block holds the expected SQLSTATE.
//! `<!-- world: NAME -->` selects the fixture that runs the examples after it, in document order.

use cassie::types::Value;

const GUIDE: &str = include_str!("../../docs/graph-guide.md");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    Success,
    Rows(Vec<String>),
    State(String),
}

#[derive(Debug, Clone)]
pub struct Example {
    pub id: String,
    pub world: String,
    pub session: String,
    pub params: Vec<Value>,
    pub sql: String,
    pub expect: Expect,
}

pub struct Parsed {
    pub examples: Vec<Example>,
    pub untagged_blocks: usize,
}

struct Pending {
    id: String,
    attributes: Vec<(String, String)>,
}

fn comment_body<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let inner = line
        .trim()
        .strip_prefix("<!--")?
        .strip_suffix("-->")?
        .trim();
    inner
        .strip_prefix(keyword)?
        .strip_prefix(':')
        .map(str::trim)
}

fn parse_pending(body: &str) -> Pending {
    let mut tokens = body.split_whitespace();
    let id = tokens.next().unwrap_or_default().to_owned();
    let attributes = tokens
        .filter_map(|token| token.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    Pending { id, attributes }
}

fn parse_params(text: &str) -> Vec<Value> {
    text.split(',')
        .map(|token| match token.strip_prefix('\'') {
            Some(rest) => Value::String(rest.trim_end_matches('\'').to_owned()),
            None => Value::Int64(token.parse().unwrap_or_default()),
        })
        .collect()
}

fn block(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    let mut body = Vec::new();
    let mut at = start + 1;
    while at < lines.len() && !lines[at].trim_start().starts_with("```") {
        body.push(lines[at].to_owned());
        at += 1;
    }
    (body, at + 1)
}

fn expectation(lines: &[&str], mut at: usize) -> (Expect, usize) {
    while at < lines.len() && lines[at].trim().is_empty() {
        at += 1;
    }
    let Some(fence) = lines.get(at).map(|line| line.trim()) else {
        return (Expect::Success, at);
    };
    let kind = match fence {
        "```text result" => true,
        "```text sqlstate" => false,
        _ => return (Expect::Success, at),
    };
    let (body, next) = block(lines, at);
    let expect = if kind {
        Expect::Rows(body.iter().map(|line| line.trim_end().to_owned()).collect())
    } else {
        Expect::State(body.join("").trim().to_owned())
    };
    (expect, next)
}

#[must_use]
pub fn parse() -> Parsed {
    let lines = GUIDE.lines().collect::<Vec<_>>();
    let mut examples = Vec::new();
    let mut untagged_blocks = 0;
    let mut world = String::new();
    let mut pending: Option<Pending> = None;
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        if let Some(name) = comment_body(line, "world") {
            name.clone_into(&mut world);
        } else if let Some(body) = comment_body(line, "example") {
            pending = Some(parse_pending(body));
        } else if line.trim() == "```sql" {
            let (body, next) = block(&lines, at);
            at = next;
            match pending.take() {
                None => untagged_blocks += 1,
                Some(tag) => {
                    let (expect, after) = expectation(&lines, at);
                    at = after;
                    let attribute = |key: &str| {
                        tag.attributes
                            .iter()
                            .find(|(name, _)| name == key)
                            .map(|(_, value)| value.clone())
                    };
                    examples.push(Example {
                        id: tag.id.clone(),
                        world: world.clone(),
                        session: attribute("session").unwrap_or_else(|| "main".to_owned()),
                        params: attribute("params")
                            .map(|text| parse_params(&text))
                            .unwrap_or_default(),
                        sql: body.join("\n"),
                        expect,
                    });
                }
            }
            continue;
        }
        at += 1;
    }
    Parsed {
        examples,
        untagged_blocks,
    }
}
